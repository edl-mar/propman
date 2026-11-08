//! Connects `store` and `disk`: load/save/reload, app state, ops. No terminal dependencies.

mod session;
mod state;

pub use session::{LoadProblem, SaveError, Session};
pub use state::{AppState, Message, update};

// The types that the user interface may read; it does not depend on `store` itself.
pub use store::{BundleAssignment, BundleId, BytesId, FileId, LineView, PathId, Store, TextId};
