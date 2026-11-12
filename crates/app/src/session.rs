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
use store::{BundleId, FileId, FileInput, FileReload, LineInput, Store};
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

/// Why a file could not be read again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadError {
    /// The id does not belong to this session.
    UnknownFile,
    /// The file is not on the disk any more.
    Deleted,
    Unreadable,
    /// The bytes cannot be decoded with the encoding that is used.
    Undecodable,
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

    /// The data, for reading. Views do not use it; they get what they show from `overview` and
    /// the counts below.
    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn file_count(&self) -> usize {
        self.store.files().len()
    }

    pub fn bundle_count(&self) -> usize {
        self.store.bundles().len()
    }

    /// The user chooses the encoding of a file. Returns whether it changed anything.
    pub fn set_encoding(&mut self, file: FileId, encoding: Encoding) -> bool {
        let mut edit = self.store.edit(EditKind::User, Source::USER);
        let changed = edit.set_encoding(file, encoding);
        edit.commit();
        changed
    }

    /// The user excludes a bundle from the session, or includes it again. Returns whether it
    /// changed anything.
    pub fn exclude_bundle(&mut self, bundle: BundleId, excluded: bool) -> bool {
        let mut edit = self.store.edit(EditKind::User, Source::USER);
        let changed = edit.exclude_bundle(bundle, excluded);
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

    /// Reads a file from the disk again. What is there becomes a new snapshot if its lines differ
    /// from the last one. An encoding that the user chose is used to read it; otherwise it is
    /// detected again.
    pub fn reload_file(&mut self, file: FileId) -> Result<(), ReloadError> {
        let (Some(path), Some(current)) = (
            self.store.file_path(file).map(Path::to_path_buf),
            self.store.file_encoding(file),
        ) else {
            return Err(ReloadError::UnknownFile);
        };
        let chosen = self
            .store
            .encoding_versions(file)
            .iter()
            .any(|version| version.source == Source::USER);
        self.read_again(file, &path, chosen.then_some(current))
            .map_err(|error| match error {
                ReadError::Io(e) if e.kind() == io::ErrorKind::NotFound => ReloadError::Deleted,
                ReadError::Io(_) => ReloadError::Unreadable,
                ReadError::Decode(_) => ReloadError::Undecodable,
            })
    }

    /// Reads the file and lets the store take what is on the disk as a new snapshot (which
    /// there is only if the lines differ). The encoding is detected unless one is given.
    fn read_again(
        &mut self,
        file: FileId,
        path: &Path,
        encoding: Option<Encoding>,
    ) -> Result<(), ReadError> {
        let loaded = read_file(path, encoding)?;
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

    /// Gives several files this encoding in one edit. Files that have it already are left out;
    /// the number of files that changed is returned.
    pub fn set_encodings(&mut self, files: &[FileId], encoding: Encoding) -> usize {
        let todo: Vec<FileId> = files
            .iter()
            .copied()
            .filter(|&file| {
                self.store
                    .file_encoding(file)
                    .is_some_and(|current| current != encoding)
            })
            .collect();
        if todo.is_empty() {
            return 0;
        }
        let mut edit = self.store.edit(EditKind::User, Source::USER);
        let changed = todo
            .iter()
            .filter(|&&file| edit.set_encoding(file, encoding))
            .count();
        edit.commit();
        changed
    }

    /// Leaves several bundles out of the session, or takes them back, in one edit. Bundles that
    /// are so already are left out; the number of bundles that changed is returned.
    pub fn exclude_bundles(&mut self, bundles: &[BundleId], excluded: bool) -> usize {
        let todo: Vec<BundleId> = bundles
            .iter()
            .copied()
            .filter(|&bundle| {
                self.store
                    .bundle_excluded(bundle)
                    .is_some_and(|current| current != excluded)
            })
            .collect();
        if todo.is_empty() {
            return 0;
        }
        let mut edit = self.store.edit(EditKind::User, Source::USER);
        let changed = todo
            .iter()
            .filter(|&&bundle| edit.exclude_bundle(bundle, excluded))
            .count();
        edit.commit();
        changed
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

        // Read the file again, with the encoding it was just written in.
        self.read_again(file, &path, Some(encoding))
            .map_err(|e| SaveError::Io(read_error(e)))
    }
}
