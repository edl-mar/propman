//! Plain value types shared by `disk` and `store`, which must not know each other.
//!
//! Only data that both sides have to carry around, with no behavior beyond what defines the
//! value itself (for example `LineEnding::as_str`). Anything with logic belongs in `disk` or
//! `store`.

use std::time::SystemTime;

/// How the bytes of a file map to text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    /// UTF-8 with a leading BOM. The BOM is not part of the decoded text.
    Utf8Bom,
    Windows1252,
    Latin1,
}

/// The ending of one physical line. `None` only occurs on an unterminated last line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    None,
    Lf,
    Cr,
    CrLf,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::None => "",
            LineEnding::Lf => "\n",
            LineEnding::Cr => "\r",
            LineEnding::CrLf => "\r\n",
        }
    }
}

/// How sure the detection of a file's bundle and locale is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Certain,
    Uncertain,
}

/// Describes the state a file had when it was read, to detect later changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fingerprint {
    pub len: u64,
    /// `None` if the platform cannot report it.
    pub modified: Option<SystemTime>,
    /// FNV-1a 64 hash of the raw bytes, including a BOM (see `disk::fingerprint::hash_bytes`).
    pub hash: u64,
}
