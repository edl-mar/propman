//! The command that runs the editor of the user on a file.
//!
//! The editor is `VISUAL`, else `EDITOR`. The value is split at white space into the program and
//! its arguments, without a shell and without quotes, and the path of the file is the last
//! argument. Nothing is assumed about the editor: there is no jump to a line, and no default if
//! none is set.

use std::ffi::OsString;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorCommand {
    pub program: String,
    pub args: Vec<OsString>,
}

/// The command for the values of `VISUAL` and `EDITOR`, or `None` if neither says anything.
pub fn editor_command(
    visual: Option<&str>,
    editor: Option<&str>,
    path: &Path,
) -> Option<EditorCommand> {
    let value = [visual, editor]
        .into_iter()
        .flatten()
        .find(|value| !value.trim().is_empty())?;
    let mut parts = value.split_whitespace();
    let program = parts.next()?.to_owned();
    let mut args: Vec<OsString> = parts.map(OsString::from).collect();
    args.push(path.as_os_str().to_owned());
    Some(EditorCommand { program, args })
}
