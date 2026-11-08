//! Domain-free index and column primitives. Knows nothing about propman.
//!
//! `Idx` lets a column address a row; the table that owns the columns is the only place that
//! creates ids. `HistoryColumn` keeps, for each row, an append-only stack of versions.

mod history_column;

pub use history_column::{HistoryColumn, Version, VersionRef};

/// An id of a table row. It gives its index but cannot be made from one: only the owner of a
/// table creates ids, so nobody can invent an id that was not handed out.
pub trait Idx: Copy {
    fn idx(self) -> usize;
}
