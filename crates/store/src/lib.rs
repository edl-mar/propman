//! Single source of truth for the in-memory data. No IO, no UI, no dependency on `disk`.

pub mod history;
mod pools;
mod raw;
mod store;

pub use pools::{BytesId, PathId, TextId};
pub use raw::{BundleId, FileId};
pub use store::{BundleAssignment, FileInput, LineInput, LineView, Store, StoreEdit};
