//! One history per value, with the source of every change recorded.
//!
//! A `Column<T>` keeps, for each row, a history of versions; the current value is the last
//! one, version 0 is the initial value. Nothing is ever removed. Every version belongs to an
//! edit (which has a kind) and carries a source (the disk, a settings layer, the user). That is
//! normally the source of its edit; only a version that a reload appends again for a higher
//! ranked source carries that source. The sources are ranked: `DISK < SHARED < LOCAL < USER`.
//!
//! - `base(source)` is a query: the latest version from that source, else version 0.
//! - `mark_saved` appends the current value again under the edit's source (a marker), so that
//!   the base of that source catches up. It appends nothing if the value is not dirty.
//! - `reload(theirs)` brings a value from the disk. If `theirs` equals the base of the disk,
//!   nothing is appended. Otherwise `(theirs, DISK)` is appended and the rule of the column
//!   decides:
//!   - `Precedence`: if a higher ranked source has a version and its latest value differs from
//!     the current one, that value is appended again (by the highest ranked such source), so
//!     the highest ranked source wins. Outcome `Kept` if it was needed, else `Taken`.
//!   - `Conflict`: the outcome is `Conflict` if the status is `Conflict` afterwards, else `Taken`.
//! - `status` is derived, never stored. It is `Conflict` only for `Conflict` columns: let `L` be
//!   the latest version; if its source is the disk, `P` the nearest earlier version from another
//!   source and `B` the value of the latest disk version before `P`, then it is a conflict if
//!   `P != B` and `P != L`. Any later version from another source resolves it.
//! - `restore` appends an old value again; `undo` restores what an edit changed. Both only
//!   append.
//!
//! Ids carrying the generation of the store are a later step and not part of this module yet.

use std::marker::PhantomData;

/// Where a change comes from. The number is the rank: a higher number wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Source(u8);

impl Source {
    pub const DISK: Source = Source(0);
    pub const SHARED: Source = Source(1);
    pub const LOCAL: Source = Source(2);
    pub const USER: Source = Source(3);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Load,
    Reload,
    SettingsRead,
    User,
    Save,
    Restore,
}

/// What happens when the disk brings a value that differs from the base.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
    Precedence,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditId(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row(usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Clean,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReloadOutcome {
    /// The disk value equals the base of the disk: nothing was appended.
    Unchanged,
    /// The new disk value is now effective (or was already).
    Taken,
    /// The new disk value was appended, then a higher ranked value was appended again.
    Kept,
    /// Both sides changed and the column asks the user.
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version<T> {
    pub value: T,
    pub edit: EditId,
    pub source: Source,
    pub kind: EditKind,
}

/// A row and the index of its last version before an edit first wrote to it.
#[derive(Debug, Clone, Copy)]
struct Touched {
    column: usize,
    row: usize,
    before: usize,
}

#[derive(Debug)]
struct EditRecord {
    kind: EditKind,
    touched: Vec<Touched>,
}

/// The log of edits; it knows the kind of each and what each one touched.
#[derive(Debug, Default)]
pub struct Journal {
    edits: Vec<EditRecord>,
    columns: usize,
}

impl Journal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn column<T: Copy + Eq>(&mut self, policy: Policy) -> Column<T> {
        let id = self.columns;
        self.columns += 1;
        Column {
            id,
            policy,
            rows: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Every change is made through an edit, which may bundle several writes.
    pub fn edit(&mut self, kind: EditKind, source: Source) -> Edit<'_> {
        let id = EditId(self.edits.len());
        self.edits.push(EditRecord {
            kind,
            touched: Vec::new(),
        });
        Edit {
            journal: self,
            id,
            source,
        }
    }

    fn kind_of(&self, edit: EditId) -> Option<EditKind> {
        self.edits.get(edit.0).map(|record| record.kind)
    }
}

pub struct Edit<'j> {
    journal: &'j mut Journal,
    id: EditId,
    source: Source,
}

impl Edit<'_> {
    /// Remembers the state of a row before this edit first wrote to it, for `undo`.
    fn touch(&mut self, column: usize, row: usize, before: usize) {
        let touched = &mut self.journal.edits[self.id.0].touched;
        if !touched.iter().any(|t| t.column == column && t.row == row) {
            touched.push(Touched {
                column,
                row,
                before,
            });
        }
    }

    /// Appends `value` unless it equals the current value. Returns whether it appended.
    pub fn set<T: Copy + Eq>(&mut self, column: &mut Column<T>, row: Row, value: T) -> bool {
        let Some(entries) = column.rows.get_mut(row.0) else {
            return false;
        };
        let Some(last) = entries.last() else {
            return false;
        };
        if last.value == value {
            return false;
        }
        let before = entries.len() - 1;
        entries.push(Entry {
            value,
            edit: self.id,
            source: self.source,
        });
        self.touch(column.id, row.0, before);
        true
    }

    /// Appends the value of an old version again. Returns false if there is no such version.
    pub fn restore<T: Copy + Eq>(
        &mut self,
        column: &mut Column<T>,
        row: Row,
        version: usize,
    ) -> bool {
        let Some(entries) = column.rows.get_mut(row.0) else {
            return false;
        };
        let Some(old) = entries.get(version).map(|e| e.value) else {
            return false;
        };
        let before = entries.len() - 1;
        entries.push(Entry {
            value: old,
            edit: self.id,
            source: self.source,
        });
        self.touch(column.id, row.0, before);
        true
    }

    /// Brings a value from the disk. The edit is expected to have the source `Source::DISK`.
    pub fn reload<T: Copy + Eq>(
        &mut self,
        column: &mut Column<T>,
        row: Row,
        theirs: T,
    ) -> ReloadOutcome {
        match column.base(self.journal, row, Source::DISK) {
            Some(base) if base != theirs => {}
            _ => return ReloadOutcome::Unchanged,
        }
        let Some(entries) = column.rows.get_mut(row.0) else {
            return ReloadOutcome::Unchanged;
        };
        let before = entries.len() - 1;
        // The version that a higher ranked source has to win with, found before appending.
        let top = entries
            .iter()
            .filter(|e| e.source > Source::DISK)
            .max_by_key(|e| e.source)
            .copied();
        entries.push(Entry {
            value: theirs,
            edit: self.id,
            source: Source::DISK,
        });
        self.touch(column.id, row.0, before);

        match column.policy {
            Policy::Precedence => match top {
                Some(top) if top.value != theirs => {
                    if let Some(entries) = column.rows.get_mut(row.0) {
                        entries.push(Entry {
                            value: top.value,
                            edit: self.id,
                            source: top.source,
                        });
                    }
                    ReloadOutcome::Kept
                }
                _ => ReloadOutcome::Taken,
            },
            Policy::Conflict => match column.status(self.journal, row) {
                Status::Conflict => ReloadOutcome::Conflict,
                Status::Clean => ReloadOutcome::Taken,
            },
        }
    }

    /// Marks the current value as saved to the source of this edit. Returns whether it
    /// appended a marker.
    pub fn mark_saved<T: Copy + Eq>(&mut self, column: &mut Column<T>, row: Row) -> bool {
        if !column.is_dirty(self.journal, row, self.source) {
            return false;
        }
        let Some(entries) = column.rows.get_mut(row.0) else {
            return false;
        };
        let Some(current) = entries.last().map(|e| e.value) else {
            return false;
        };
        entries.push(Entry {
            value: current,
            edit: self.id,
            source: self.source,
        });
        true
    }

    /// Restores what the edit `target` changed, in all given columns, by appending.
    pub fn undo(&mut self, target: EditId, columns: &mut [&mut dyn Restorable]) {
        for column in columns {
            column.undo(self, target);
        }
    }

    pub fn commit(self) -> EditId {
        self.id
    }
}

/// A column of any value type that can take part in `Edit::undo`.
pub trait Restorable {
    fn undo(&mut self, edit: &mut Edit<'_>, target: EditId);
}

#[derive(Debug, Clone, Copy)]
struct Entry<T> {
    value: T,
    edit: EditId,
    source: Source,
}

pub struct Column<T> {
    id: usize,
    policy: Policy,
    rows: Vec<Vec<Entry<T>>>,
    _marker: PhantomData<T>,
}

impl<T: Copy + Eq> Column<T> {
    /// Adds a row with `initial` as version 0.
    pub fn add_row(&mut self, edit: &mut Edit<'_>, initial: T) -> Row {
        self.rows.push(vec![Entry {
            value: initial,
            edit: edit.id,
            source: edit.source,
        }]);
        Row(self.rows.len() - 1)
    }

    pub fn current(&self, row: Row) -> Option<T> {
        self.rows.get(row.0)?.last().map(|e| e.value)
    }

    pub fn versions(&self, journal: &Journal, row: Row) -> Vec<Version<T>> {
        let Some(entries) = self.rows.get(row.0) else {
            return Vec::new();
        };
        entries
            .iter()
            .filter_map(|e| {
                Some(Version {
                    value: e.value,
                    edit: e.edit,
                    source: e.source,
                    kind: journal.kind_of(e.edit)?,
                })
            })
            .collect()
    }

    /// The latest version from `source`, else version 0.
    pub fn base(&self, _journal: &Journal, row: Row, source: Source) -> Option<T> {
        let entries = self.rows.get(row.0)?;
        entries
            .iter()
            .rev()
            .find(|e| e.source == source)
            .or_else(|| entries.first())
            .map(|e| e.value)
    }

    /// Whether the current value differs from the base of `source`.
    pub fn is_dirty(&self, journal: &Journal, row: Row, source: Source) -> bool {
        match (self.current(row), self.base(journal, row, source)) {
            (Some(current), Some(base)) => current != base,
            _ => false,
        }
    }

    pub fn status(&self, _journal: &Journal, row: Row) -> Status {
        if self.policy == Policy::Precedence {
            return Status::Clean;
        }
        let Some(entries) = self.rows.get(row.0) else {
            return Status::Clean;
        };
        let Some(last) = entries.last() else {
            return Status::Clean;
        };
        if last.source != Source::DISK {
            return Status::Clean;
        }
        // P: the nearest earlier version from another source
        let Some(p) = entries.iter().rposition(|e| e.source != Source::DISK) else {
            return Status::Clean;
        };
        // B: the latest disk version before P, else version 0
        let base = entries[..p]
            .iter()
            .rev()
            .find(|e| e.source == Source::DISK)
            .or_else(|| entries.first())
            .map(|e| e.value);
        let ours = entries[p].value;
        if Some(ours) != base && ours != last.value {
            Status::Conflict
        } else {
            Status::Clean
        }
    }
}

impl<T: Copy + Eq> Restorable for Column<T> {
    fn undo(&mut self, edit: &mut Edit<'_>, target: EditId) {
        let slots: Vec<Touched> = edit
            .journal
            .edits
            .get(target.0)
            .map(|record| {
                record
                    .touched
                    .iter()
                    .filter(|t| t.column == self.id)
                    .copied()
                    .collect()
            })
            .unwrap_or_default();
        for slot in slots {
            let Some(entries) = self.rows.get_mut(slot.row) else {
                continue;
            };
            let Some(old) = entries.get(slot.before).map(|e| e.value) else {
                continue;
            };
            let before = entries.len() - 1;
            entries.push(Entry {
                value: old,
                edit: edit.id,
                source: edit.source,
            });
            edit.touch(self.id, slot.row, before);
        }
    }
}
