//! Reads and writes a whole `.properties` file by chaining the lower layers.
//!
//! `read_file` = read bytes, `encoding::decode`. The caller splits the text with
//! `lines::split_lines`. `write_file` = `lines::join_lines`, `encoding::encode`,
//! `atomic::atomic_write`.
//!
//! Invariant (the golden roundtrip): reading a file and writing the unchanged lines back
//! produces the same bytes, whatever the encoding, BOM or line endings. Changing one line
//! changes only the bytes of that line.

use std::io;
use std::path::Path;

use crate::atomic::atomic_write;
use crate::encoding::{DecodeError, Decoded, Encoding, decode, encode};
use crate::lines::{Line, join_lines};

#[derive(Debug)]
pub enum ReadError {
    Io(io::Error),
    /// Only possible when a hint forces an encoding that cannot represent the bytes.
    Decode(DecodeError),
}

/// `hint` forces an encoding instead of detecting it.
pub fn read_file(path: &Path, hint: Option<Encoding>) -> Result<Decoded, ReadError> {
    let bytes = std::fs::read(path).map_err(ReadError::Io)?;
    decode(&bytes, hint).map_err(ReadError::Decode)
}

/// Characters the encoding cannot represent are written as `\uXXXX` escapes.
pub fn write_file(path: &Path, encoding: Encoding, lines: &[Line<'_>]) -> io::Result<()> {
    atomic_write(path, &encode(&join_lines(lines), encoding))
}
