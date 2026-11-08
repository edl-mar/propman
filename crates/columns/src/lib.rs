//! Domain-free index and column primitives. Knows nothing about propman.
//!
//! `Idx` lets a column address a row; the table that owns the columns is the only place that
//! creates ids. Every id carries the `Generation` of the store it was made in, and a column
//! only knows ids of its own generation. `HistoryColumn` keeps, for each row, an append-only
//! stack of versions; `Pool` stores large values once, with stable ids.

use std::sync::atomic::{AtomicU64, Ordering};

mod history_column;
mod pool;

pub use history_column::{HistoryColumn, Version, VersionRef};
pub use pool::Pool;

/// The number of one store. Every call to `Generation::next` returns a number that no other
/// call has returned, so ids of different stores can never be mistaken for each other: not
/// after a rebuild, and not between two stores that exist at the same time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Generation(u64);

impl Generation {
    pub fn next() -> Generation {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        Generation(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

/// An id of a table row. It gives its index and its generation but cannot be made from them:
/// only the owner of a table creates ids, so nobody can invent an id that was not handed out.
pub trait Idx: Copy {
    fn idx(self) -> usize;
    fn generation(self) -> Generation;
}
