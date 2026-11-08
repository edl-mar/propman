//! A column with an append-only stack of versions per row.
//!
//! The current value of a row is its last version; version 0 is the initial value. Nothing is
//! ever removed or changed. Every version carries a small piece of metadata `M` that the column
//! does not interpret (the store uses it for the edit and the source). Values and metadata are
//! small and `Copy`; large values live in pools and are referred to by id.
//!
//! The column returns plain indices when it creates a row (like a push) and never makes an id
//! itself: the owner of the table turns the index into its own id type, so that all columns of
//! one table can share that id.

use std::marker::PhantomData;

use crate::Idx;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version<T, M> {
    pub value: T,
    pub meta: M,
}

/// Refers to one version of one row, for example "the initial value of this file's encoding".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VersionRef<Id> {
    pub id: Id,
    pub version: usize,
}

pub struct HistoryColumn<Id, T, M> {
    rows: Vec<Vec<Version<T, M>>>,
    _id: PhantomData<Id>,
}

impl<Id: Idx, T: Copy, M: Copy> HistoryColumn<Id, T, M> {
    pub fn new() -> Self {
        Self {
            rows: Vec::new(),
            _id: PhantomData,
        }
    }

    /// The number of rows.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Adds a row whose version 0 is `initial`, and returns its index.
    pub fn push_row(&mut self, initial: T, meta: M) -> usize {
        self.rows.push(vec![Version {
            value: initial,
            meta,
        }]);
        self.rows.len() - 1
    }

    /// Appends a version to a row and returns its number, or `None` if the row does not exist.
    pub fn append(&mut self, id: Id, value: T, meta: M) -> Option<usize> {
        let versions = self.rows.get_mut(id.idx())?;
        versions.push(Version { value, meta });
        Some(versions.len() - 1)
    }

    /// The last version's value.
    pub fn current(&self, id: Id) -> Option<T> {
        self.rows.get(id.idx())?.last().map(|v| v.value)
    }

    /// All versions of a row, oldest first; empty if the row does not exist.
    pub fn versions(&self, id: Id) -> &[Version<T, M>] {
        self.rows.get(id.idx()).map_or(&[], Vec::as_slice)
    }

    pub fn version_count(&self, id: Id) -> usize {
        self.versions(id).len()
    }

    /// A reference to the last version.
    pub fn current_ref(&self, id: Id) -> Option<VersionRef<Id>> {
        let version = self.version_count(id).checked_sub(1)?;
        Some(VersionRef { id, version })
    }

    pub fn at(&self, reference: VersionRef<Id>) -> Option<Version<T, M>> {
        self.versions(reference.id).get(reference.version).copied()
    }

    /// The newest version whose metadata satisfies `matches`, with its number.
    pub fn latest_where(
        &self,
        id: Id,
        matches: impl Fn(&M) -> bool,
    ) -> Option<(usize, Version<T, M>)> {
        let versions = self.versions(id);
        let number = versions.iter().rposition(|v| matches(&v.meta))?;
        Some((number, versions[number]))
    }
}

impl<Id: Idx, T: Copy, M: Copy> Default for HistoryColumn<Id, T, M> {
    fn default() -> Self {
        Self::new()
    }
}
