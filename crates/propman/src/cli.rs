//! The command line: `propman [FOLDER]`.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    TooManyArguments,
    /// An argument that starts with `-`.
    UnknownOption(OsString),
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UsageError::TooManyArguments => write!(f, "too many arguments"),
            UsageError::UnknownOption(option) => {
                write!(f, "unknown option {}", option.to_string_lossy())
            }
        }
    }
}

/// The folder to open, from the arguments after the name of the program. Without an argument it
/// is the current folder.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<PathBuf, UsageError> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(PathBuf::from("."));
    };
    if first.as_encoded_bytes().first() == Some(&b'-') {
        return Err(UsageError::UnknownOption(first));
    }
    if args.next().is_some() {
        return Err(UsageError::TooManyArguments);
    }
    Ok(PathBuf::from(first))
}

pub fn usage() -> &'static str {
    "usage: propman [FOLDER]"
}
