//! A session: the store, filled from a folder on the disk, and what `app` does with it.
//!
//! `open` finds the `.properties` files below a folder, classifies them into bundles and
//! locales, reads each one and loads it into the store, one edit of the kind `Load` per file.
//! Problems (an unreadable folder, a file that cannot be read) are collected and do not stop
//! the rest from loading.
//!
//! `save_file` writes the lines of a file back with the current encoding, expecting the file on
//! the disk to be the one that was read (else it is refused), then reads the file again and lets
//! the store take what is now on the disk as a new snapshot. So after a save the store shows what
//! is on the disk, also if a character had to be written as an escape.
//!
//! Whether a file has to be saved is decided by comparing the bytes that its lines would give
//! with the bytes of the disk, not by the chosen encoding: a choice can be invisible on the disk
//! (a file with only ASCII looks the same in every encoding) and then only lives in the settings.

use std::io;
use std::path::{Path, PathBuf};

use disk::bundles::{Classified, classify_files};
use disk::discover::{DiscoverOptions, discover};
use disk::encoding::encode;
use disk::file::{Loaded, ReadError, WriteError, read_file, write_file};
use disk::lines::{Line, join_lines, split_lines};
use store::history::{EditKind, Source};
use store::{FileId, FileInput, FileReload, LineInput, Store};
use types::Encoding;

/// Something that went wrong while loading, without stopping the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadProblem {
    /// The entry that could not be read, if known.
    pub path: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug)]
pub enum SaveError {
    /// The id does not belong to this session.
    UnknownFile,
    /// The file on the disk is not the one that was read. Nothing was written.
    Changed,
    /// The file was deleted since it was read. Nothing was written; it is not recreated.
    Deleted,
    Io(io::Error),
}

pub struct Session {
    store: Store,
    root: PathBuf,
}

fn line_inputs<'a>(lines: &[Line<'a>]) -> Vec<LineInput<'a>> {
    lines
        .iter()
        .map(|line| LineInput {
            content: line.content,
            ending: line.ending,
        })
        .collect()
}

fn read_error(error: ReadError) -> io::Error {
    match error {
        ReadError::Io(e) => e,
        ReadError::Decode(e) => io::Error::new(io::ErrorKind::InvalidData, format!("{e:?}")),
    }
}

/// Loads one file that was read and classified into the store, as one edit of the kind `Load`.
fn load_file(store: &mut Store, classified: &Classified, loaded: &Loaded) {
    let lines = split_lines(&loaded.decoded.text);
    let inputs = line_inputs(&lines);
    let input = FileInput {
        path: &classified.path,
        bytes: &loaded.bytes,
        encoding: loaded.decoded.encoding,
        fingerprint: loaded.fingerprint,
        lines: &inputs,
        bundle: &classified.bundle,
        locale: classified.locale.as_deref(),
        confidence: classified.confidence,
    };
    let mut edit = store.edit(EditKind::Load, Source::DISK);
    edit.add_file(input);
    edit.commit();
}

impl Session {
    /// Loads everything below `root`.
    pub fn open(root: &Path) -> (Session, Vec<LoadProblem>) {
        let mut store = Store::new();
        let mut problems = Vec::new();

        let discovery = discover(root, &DiscoverOptions::default());
        for error in discovery.errors {
            problems.push(LoadProblem {
                path: error.path,
                message: error.message,
            });
        }
        for classified in classify_files(&discovery.files) {
            match read_file(&classified.path, None) {
                Ok(loaded) => load_file(&mut store, &classified, &loaded),
                Err(error) => problems.push(LoadProblem {
                    path: Some(classified.path.clone()),
                    message: read_error(error).to_string(),
                }),
            }
        }

        let session = Session {
            store,
            root: root.to_path_buf(),
        };
        (session, problems)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The data, for reading.
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// The user chooses the encoding of a file. Returns whether it changed anything.
    pub fn set_encoding(&mut self, file: FileId, encoding: Encoding) -> bool {
        let mut edit = self.store.edit(EditKind::User, Source::USER);
        let changed = edit.set_encoding(file, encoding);
        edit.commit();
        changed
    }

    /// The files whose lines, encoded with their current encoding, differ from the bytes on the
    /// disk as they were read.
    pub fn dirty_files(&self) -> Vec<FileId> {
        self.store
            .files()
            .into_iter()
            .filter(|&file| self.needs_writing(file))
            .collect()
    }

    /// Whether the bytes that the lines of a file would give differ from the bytes of the disk.
    fn needs_writing(&self, file: FileId) -> bool {
        let (Some(encoding), Some(bytes)) =
            (self.store.file_encoding(file), self.store.file_bytes(file))
        else {
            return false;
        };
        let views = self.store.file_lines(file);
        let lines: Vec<Line<'_>> = views
            .iter()
            .map(|view| Line {
                content: view.content,
                ending: view.ending,
            })
            .collect();
        encode(&join_lines(&lines), encoding) != bytes
    }

    /// Writes a file, whether it is dirty or not.
    pub fn save_file(&mut self, file: FileId) -> Result<(), SaveError> {
        let (Some(path), Some(encoding), Some(expected)) = (
            self.store.file_path(file).map(Path::to_path_buf),
            self.store.file_encoding(file),
            self.store.file_fingerprint(file),
        ) else {
            return Err(SaveError::UnknownFile);
        };

        {
            let views = self.store.file_lines(file);
            let lines: Vec<Line<'_>> = views
                .iter()
                .map(|view| Line {
                    content: view.content,
                    ending: view.ending,
                })
                .collect();
            write_file(&path, encoding, &lines, Some(&expected)).map_err(|error| match error {
                WriteError::Io(e) => SaveError::Io(e),
                WriteError::Changed => SaveError::Changed,
                WriteError::Deleted => SaveError::Deleted,
            })?;
        }

        // Read the file again, with the encoding it was just written in, and let the store take
        // what is now on the disk as a new snapshot.
        let loaded = read_file(&path, Some(encoding)).map_err(|e| SaveError::Io(read_error(e)))?;
        let lines = split_lines(&loaded.decoded.text);
        let inputs = line_inputs(&lines);
        let input = FileReload {
            bytes: &loaded.bytes,
            encoding: loaded.decoded.encoding,
            fingerprint: loaded.fingerprint,
            lines: &inputs,
        };
        let mut edit = self.store.edit(EditKind::Reload, Source::DISK);
        edit.reload_file(file, input);
        edit.commit();
        Ok(())
    }
}
