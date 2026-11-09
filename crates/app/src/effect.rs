//! What `update` asks the outside world to do: things that touch the disk or the terminal.
//!
//! An effect is plain data. `update` returns at most one with the new state, `perform` carries it
//! out, and what came of it is told as a message. An order of several steps is therefore the
//! chain of messages and effects, not a list: the next step is decided when the result of the
//! last one is known.
//!
//! `perform` does the disk part itself. The part that needs the terminal, running the editor of
//! the user, is handed in as a function, because only the user interface knows how to leave and
//! enter the terminal.

use std::path::{Path, PathBuf};

use store::FileId;

use crate::state::{AppState, Message};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Open the file in the editor of the user and wait until it is closed.
    Edit { file: FileId, path: PathBuf },
    /// Read the file from the disk again; what is there is a new snapshot if the lines differ.
    Reload(FileId),
}

/// How the editor of the user ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorOutcome {
    Done,
    /// Neither `VISUAL` nor `EDITOR` is set.
    NoEditor,
    CouldNotStart,
    /// The editor ended with an error; its exit code, if it has one.
    Failed(Option<i32>),
}

/// Carries out an effect and returns the state with a message that tells the result. `edit` runs
/// the editor on a path and is only called for `Effect::Edit`.
pub fn perform(
    mut state: AppState,
    effect: Effect,
    edit: impl FnOnce(&Path) -> EditorOutcome,
) -> (AppState, Message) {
    let message = match effect {
        Effect::Edit { file, path } => Message::EditorClosed {
            file,
            outcome: edit(&path),
        },
        Effect::Reload(file) => Message::Reloaded {
            file,
            result: state.session_mut().reload_file(file),
        },
    };
    (state, message)
}
