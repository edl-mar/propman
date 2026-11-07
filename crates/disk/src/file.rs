//! Reads and writes a whole `.properties` file by chaining the lower layers.
//!
//! `read_file` = stamp, read bytes, hash, `encoding::decode`. The caller splits the text with
//! `lines::split_lines`. `write_file` = optional change check, `lines::join_lines`,
//! `encoding::encode`, `atomic::atomic_write`, new fingerprint.
//!
//! Invariant (the golden roundtrip): reading a file and writing the unchanged lines back
//! produces the same bytes, whatever the encoding, BOM or line endings. Changing one line
//! changes only the bytes of that line.
//!
//! The fingerprint returned by `read_file` and `write_file` describes the bytes as they are on
//! disk, so a caller can hand it to the next `write_file` to detect outside changes.

use std::fs;
use std::io;
use std::path::Path;

use crate::atomic::atomic_write;
use crate::encoding::{DecodeError, Decoded, Encoding, decode, encode};
use crate::fingerprint::{Check, Fingerprint, check, hash_bytes, read_with_fingerprint};
use crate::lines::{Line, join_lines};

#[derive(Debug)]
pub struct Loaded {
    pub decoded: Decoded,
    pub fingerprint: Fingerprint,
}

#[derive(Debug)]
pub enum ReadError {
    Io(io::Error),
    /// Only possible when a hint forces an encoding that cannot represent the bytes.
    Decode(DecodeError),
}

#[derive(Debug)]
pub enum WriteError {
    Io(io::Error),
    /// The content on disk is not the one described by the expected fingerprint. Nothing was
    /// written.
    Changed,
    /// The file was deleted since it was read. Nothing was written (it is not recreated).
    Deleted,
}

/// `hint` forces an encoding instead of detecting it.
pub fn read_file(path: &Path, hint: Option<Encoding>) -> Result<Loaded, ReadError> {
    let (bytes, fingerprint) = read_with_fingerprint(path).map_err(ReadError::Io)?;
    let decoded = decode(&bytes, hint).map_err(ReadError::Decode)?;
    Ok(Loaded {
        decoded,
        fingerprint,
    })
}

/// With `expected`, the file is only replaced if its content still matches; without it
/// (a new file, or a deliberate overwrite) nothing is checked. Characters the encoding cannot
/// represent are written as `\uXXXX` escapes. Returns the fingerprint of the written file.
pub fn write_file(
    path: &Path,
    encoding: Encoding,
    lines: &[Line<'_>],
    expected: Option<&Fingerprint>,
) -> Result<Fingerprint, WriteError> {
    if let Some(expected) = expected {
        match check(path, expected).map_err(WriteError::Io)? {
            Check::Unchanged(_) => {}
            Check::Changed => return Err(WriteError::Changed),
            Check::Deleted => return Err(WriteError::Deleted),
        }
    }
    let bytes = encode(&join_lines(lines), encoding);
    atomic_write(path, &bytes).map_err(WriteError::Io)?;
    let modified = fs::metadata(path).and_then(|m| m.modified()).ok();
    Ok(Fingerprint {
        len: bytes.len() as u64,
        modified,
        hash: hash_bytes(&bytes),
    })
}
