//! Connects `store` and `disk`: load/save/reload, app state, ops. No terminal dependencies.

mod commands;
mod effect;
mod list;
mod overview;
mod preview;
mod selection;
mod session;
mod state;
mod text;

pub use commands::{Action, Candidate, Command, Entry, Value, candidates, commands, encoding_name};
pub use effect::{EditorOutcome, Effect, perform};
pub use list::{ListState, Movement};
pub use overview::{ItemId, OverviewItem, overview};
pub use preview::{Preview, PreviewLine, preview};
pub use selection::{applicable, selection, summary, target};
pub use session::{LoadProblem, ReloadError, SaveError, Session};
pub use state::{AppState, CommandLine, Message, OverviewState, View, update};
pub use text::{TextEdit, TextField};

// The types that the user interface may read; it does not depend on `store` or `types` itself.
pub use store::{BundleAssignment, BundleId, FileId, LineView, Store};
pub use types::{Confidence, Encoding};
