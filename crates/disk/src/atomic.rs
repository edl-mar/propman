//! Replaces a file's content so that it is never seen half written.
//!
//! Writes a hidden temporary file next to the target (same file system, so the final rename
//! is atomic), syncs it, then renames it over the target. If anything fails the target is left
//! as it was and the temporary file is removed. A leftover after a crash is hidden, so
//! `discover` never lists it.
//!
//! Knows nothing about encodings, line endings or file formats: it gets finished bytes.
//! A read-only target is an error, never overridden. Permissions of the replaced file are kept
//! on Unix; owner and Windows ACLs are not. Symlinks are not treated specially. It does not
//! detect that the file was changed by someone else since it was read (that is up to `app`).
//! Missing parent directories are an error and are not created.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Hidden, unique per process and call, next to the target.
fn temp_path(target: &Path) -> io::Result<PathBuf> {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    // A bare file name has the empty path as parent.
    let dir = match target.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let mut temp = OsString::from(".");
    temp.push(name);
    temp.push(format!(
        ".propman-tmp-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    Ok(dir.join(temp))
}

fn write_temp(temp: &Path, bytes: &[u8], replaced: Option<&fs::Metadata>) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(temp)?;
    file.write_all(bytes)?;
    if let Some(metadata) = replaced {
        file.set_permissions(metadata.permissions())?;
    }
    file.sync_all()
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    // Checked before anything is written, so that a refused target costs nothing.
    let replaced = match fs::metadata(path) {
        Ok(m) if m.is_dir() => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "target is a directory",
            ));
        }
        Ok(m) if m.permissions().readonly() => {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "target is read-only",
            ));
        }
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };

    let temp = temp_path(path)?;
    let result = write_temp(&temp, bytes, replaced.as_ref()).and_then(|()| fs::rename(&temp, path));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
        return result;
    }

    // Make the rename itself durable. Best effort: not every file system allows opening a
    // directory, and Windows has no equivalent.
    #[cfg(unix)]
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let _ = fs::File::open(dir).and_then(|d| d.sync_all());
    }
    Ok(())
}
