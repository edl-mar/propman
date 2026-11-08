//! Connects `store` and `disk`: load/save/reload, app state, ops. No terminal dependencies.

mod overview;
mod session;
mod state;

pub use overview::{OverviewItem, overview};
pub use session::{LoadProblem, SaveError, Session};
pub use state::{AppState, Message, OverviewState, View, update};

// The types that the user interface may read; it does not depend on `store` or `types` itself.
pub use store::{BundleAssignment, BundleId, FileId, LineView, Store};
pub use types::{Confidence, Encoding};
