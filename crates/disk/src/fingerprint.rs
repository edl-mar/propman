//! Detects whether a file changed on disk since it was read.
//!
//! A `Fingerprint` is a stamp (length and modification time, taken before the bytes are read)
//! plus a hash of the raw bytes that were read. The hash is the authority: the modification
//! time alone gives false alarms (`git checkout` resets it) and misses changes within the
//! file system's time resolution. The stamp is kept for cheap polling later.
//!
//! The hash is FNV-1a 64, written by hand on purpose: its result is fixed by the algorithm, so
//! a fingerprint may one day be saved and compared across runs and Rust versions
//! (`DefaultHasher` makes no such promise). If the algorithm ever changes, add a tag.
//! A small race remains between a check and the following write; it cannot be closed
//! without locking.

use std::fs;
use std::io;
use std::path::Path;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint {
    pub len: u64,
    /// `None` if the platform cannot report it.
    pub modified: Option<SystemTime>,
    /// `hash_bytes` of the raw bytes, including a BOM.
    pub hash: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Check {
    /// The content is the one that was read. Carries the fingerprint with the stamp as it is
    /// now, which differs from the expected one if the file was only touched.
    Unchanged(Fingerprint),
    Changed,
    Deleted,
}

/// FNV-1a, 64 bit.
pub fn hash_bytes(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET_BASIS, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(PRIME)
    })
}

/// Reads `path` and fingerprints exactly the bytes returned. The modification time is taken
/// *before* reading: if the file changes in between, the stamp is older than the content, which
/// can only cause a false alarm later, never a missed change.
pub(crate) fn read_with_fingerprint(path: &Path) -> io::Result<(Vec<u8>, Fingerprint)> {
    let modified = fs::metadata(path)?.modified().ok();
    let bytes = fs::read(path)?;
    let fingerprint = Fingerprint {
        len: bytes.len() as u64,
        modified,
        hash: hash_bytes(&bytes),
    };
    Ok((bytes, fingerprint))
}

/// Compares the content of `path` with `expected` by hash. Errors other than a missing file
/// (for example a directory, or no permission) are returned as they are.
pub fn check(path: &Path, expected: &Fingerprint) -> io::Result<Check> {
    let current = match read_with_fingerprint(path) {
        Ok((_, current)) => current,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Check::Deleted),
        Err(e) => return Err(e),
    };
    if current.hash == expected.hash && current.len == expected.len {
        Ok(Check::Unchanged(current))
    } else {
        Ok(Check::Changed)
    }
}
