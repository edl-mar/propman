//! Finds the `.properties` files below a directory.
//!
//! Only looks at the file system tree, never reads file contents. Hidden files and
//! directories (name starting with `.`, or the Windows "hidden" attribute) are skipped
//! together with everything below them; the root itself is never judged. Problems are
//! reported in `Discovery::errors` instead of being swallowed, and do not stop the walk.
//! The result is sorted by path and does not depend on the order the file system lists entries.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use walkdir::{DirEntry, WalkDir};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoverOptions {
    /// `false` lists only the direct children of the root.
    pub recursive: bool,
}

impl Default for DiscoverOptions {
    fn default() -> Self {
        Self { recursive: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverError {
    /// The entry that could not be read, if known.
    pub path: Option<PathBuf>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discovery {
    /// Paths start with `root` as given; sorted.
    pub files: Vec<PathBuf>,
    pub errors: Vec<DiscoverError>,
}

fn error(path: &Path, message: impl ToString) -> DiscoverError {
    DiscoverError {
        path: Some(path.to_path_buf()),
        message: message.to_string(),
    }
}

fn is_hidden(entry: &DirEntry) -> bool {
    if entry.file_name().to_string_lossy().starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if let Ok(metadata) = entry.metadata() {
            return metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0;
        }
    }
    false
}

fn is_properties_file(entry: &DirEntry) -> bool {
    entry.file_type().is_file() && entry.path().extension() == Some(OsStr::new("properties"))
}

pub fn discover(root: &Path, options: &DiscoverOptions) -> Discovery {
    let mut files = Vec::new();
    let mut errors = Vec::new();

    // Checked up front so that a missing root or a root that is a file is one clear error.
    match std::fs::metadata(root) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => {
            errors.push(error(root, "not a directory"));
            return Discovery { files, errors };
        }
        Err(e) => {
            errors.push(error(root, e));
            return Discovery { files, errors };
        }
    }

    // Following symlinks is safe: walkdir reports a loop as an error instead of walking forever.
    let mut walker = WalkDir::new(root).follow_links(true).sort_by_file_name();
    if !options.recursive {
        walker = walker.max_depth(1);
    }
    // The root (depth 0) is never judged: `.`, `..` and hidden parents are fine there.
    let walker = walker
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(e));

    for entry in walker {
        match entry {
            Ok(e) if is_properties_file(&e) => files.push(e.into_path()),
            Ok(_) => {}
            Err(e) => errors.push(DiscoverError {
                path: e.path().map(Path::to_path_buf),
                message: e.to_string(),
            }),
        }
    }

    // walkdir sorts per directory by file name; sorting whole paths makes the order explicit.
    files.sort();
    Discovery { files, errors }
}
