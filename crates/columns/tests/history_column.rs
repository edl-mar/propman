//! Specification of the generic history column (see `columns::HistoryColumn`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use columns::{Generation, HistoryColumn, Idx, Version, VersionRef};

// ── A table that owns its ids, as the store will ─────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct FileId(Generation, usize);

impl Idx for FileId {
    fn idx(self) -> usize {
        self.1
    }

    fn generation(self) -> Generation {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Enc {
    Latin1,
    Utf8,
    Windows1252,
}

/// Small metadata per version; the column does not interpret it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Meta {
    edit: u32,
    from_user: bool,
}

const LOAD: Meta = Meta {
    edit: 0,
    from_user: false,
};

fn by_user(edit: u32) -> Meta {
    Meta {
        edit,
        from_user: true,
    }
}

type Column = HistoryColumn<FileId, Enc, Meta>;

fn new_column() -> Column {
    Column::new(Generation::next())
}

/// The owner of the ids: only it turns an index into a `FileId`.
fn add(column: &mut Column, initial: Enc) -> FileId {
    let generation = column.generation();
    FileId(generation, column.push_row(initial, LOAD))
}

// ── Rows and versions ────────────────────────────────────────────────────────

#[test]
fn a_new_column_has_no_rows() {
    let column = new_column();
    assert_eq!(column.len(), 0);
    assert!(column.is_empty());
}

#[test]
fn a_new_row_starts_with_its_initial_value_as_version_0() {
    let mut column = new_column();
    let first = add(&mut column, Enc::Latin1);
    let second = add(&mut column, Enc::Utf8);

    assert_eq!((first.idx(), second.idx()), (0, 1));
    assert_eq!(column.len(), 2);
    assert!(!column.is_empty());
    assert_eq!(column.current(first), Some(Enc::Latin1));
    assert_eq!(column.current(second), Some(Enc::Utf8));
    assert_eq!(
        column.versions(first),
        [Version {
            value: Enc::Latin1,
            meta: LOAD
        }]
    );
}

#[test]
fn appending_adds_a_version_and_makes_it_current() {
    let mut column = new_column();
    let id = add(&mut column, Enc::Latin1);

    assert_eq!(column.append(id, Enc::Utf8, by_user(1)), Some(1));
    assert_eq!(column.append(id, Enc::Windows1252, by_user(2)), Some(2));

    assert_eq!(column.current(id), Some(Enc::Windows1252));
    assert_eq!(column.version_count(id), 3);
    let values: Vec<Enc> = column.versions(id).iter().map(|v| v.value).collect();
    assert_eq!(values, [Enc::Latin1, Enc::Utf8, Enc::Windows1252]);
    assert_eq!(column.versions(id)[1].meta, by_user(1));
}

#[test]
fn appending_the_same_value_again_is_still_a_version() {
    // The column does not judge values; the rules above it decide what is worth appending.
    let mut column = new_column();
    let id = add(&mut column, Enc::Latin1);
    assert_eq!(column.append(id, Enc::Latin1, by_user(1)), Some(1));
    assert_eq!(column.version_count(id), 2);
}

#[test]
fn rows_are_independent() {
    let mut column = new_column();
    let first = add(&mut column, Enc::Latin1);
    let second = add(&mut column, Enc::Latin1);

    column.append(first, Enc::Utf8, by_user(1));

    assert_eq!(column.version_count(first), 2);
    assert_eq!(column.version_count(second), 1);
    assert_eq!(column.current(second), Some(Enc::Latin1));
}

#[test]
fn an_unknown_row_gives_nothing_and_changes_nothing() {
    let mut column = new_column();
    let known = add(&mut column, Enc::Latin1);
    let unknown = FileId(column.generation(), 7);

    assert_eq!(column.current(unknown), None);
    assert_eq!(column.version_count(unknown), 0);
    assert!(column.versions(unknown).is_empty());
    assert_eq!(column.current_ref(unknown), None);
    assert_eq!(column.append(unknown, Enc::Utf8, by_user(1)), None);
    assert_eq!(column.latest_where(unknown, |_| true), None);
    assert_eq!(column.len(), 1);
    assert_eq!(column.version_count(known), 1);
}

#[test]
fn nothing_is_ever_removed() {
    let mut column = new_column();
    let id = add(&mut column, Enc::Latin1);
    let mut counts = vec![column.version_count(id)];
    for edit in 1..=20 {
        column.append(id, Enc::Utf8, by_user(edit));
        counts.push(column.version_count(id));
    }
    assert!(counts.windows(2).all(|pair| pair[1] == pair[0] + 1));
    assert_eq!(column.versions(id)[0].value, Enc::Latin1);
}

// ── References to versions ───────────────────────────────────────────────────

#[test]
fn a_reference_addresses_one_version_of_one_row() {
    let mut column = new_column();
    let id = add(&mut column, Enc::Latin1);
    column.append(id, Enc::Utf8, by_user(1));

    let initial = VersionRef { id, version: 0 };
    let second = VersionRef { id, version: 1 };

    assert_eq!(
        column.at(initial),
        Some(Version {
            value: Enc::Latin1,
            meta: LOAD
        })
    );
    assert_eq!(
        column.at(second),
        Some(Version {
            value: Enc::Utf8,
            meta: by_user(1)
        })
    );
    assert_eq!(column.at(VersionRef { id, version: 2 }), None);
    assert_eq!(
        column.at(VersionRef {
            id: FileId(column.generation(), 9),
            version: 0
        }),
        None
    );
}

#[test]
fn a_reference_to_the_current_version_keeps_pointing_at_that_version() {
    let mut column = new_column();
    let id = add(&mut column, Enc::Latin1);

    let reference = column.current_ref(id).unwrap();
    assert_eq!(reference, VersionRef { id, version: 0 });

    column.append(id, Enc::Utf8, by_user(1));

    assert_eq!(column.at(reference).map(|v| v.value), Some(Enc::Latin1));
    assert_eq!(column.current_ref(id), Some(VersionRef { id, version: 1 }));
}

// ── Finding a version by its metadata ────────────────────────────────────────

#[test]
fn the_latest_matching_version_is_found_with_its_number() {
    let mut column = new_column();
    let id = add(&mut column, Enc::Latin1); // version 0, not by the user
    column.append(id, Enc::Utf8, by_user(1)); // 1
    column.append(id, Enc::Windows1252, by_user(2)); // 2
    column.append(
        id,
        Enc::Latin1,
        Meta {
            edit: 3,
            from_user: false,
        },
    ); // 3

    let (number, version) = column.latest_where(id, |m| m.from_user).unwrap();
    assert_eq!(number, 2);
    assert_eq!(version.value, Enc::Windows1252);

    let (number, _) = column.latest_where(id, |m| !m.from_user).unwrap();
    assert_eq!(number, 3);
}

#[test]
fn a_criterion_can_use_any_part_of_the_metadata() {
    let mut column = new_column();
    let id = add(&mut column, Enc::Latin1);
    column.append(id, Enc::Utf8, by_user(5));
    column.append(id, Enc::Windows1252, by_user(6));

    let found = column.latest_where(id, |m| m.edit == 5).unwrap();
    assert_eq!(found.0, 1);
    assert_eq!(found.1.value, Enc::Utf8);
}

#[test]
fn no_matching_version_gives_none() {
    let mut column = new_column();
    let id = add(&mut column, Enc::Latin1);
    assert_eq!(column.latest_where(id, |m| m.from_user), None);
}

// ── Several columns of one table share one id ────────────────────────────────

struct FileTable {
    generation: Generation,
    encoding: HistoryColumn<FileId, Enc, Meta>,
    excluded: HistoryColumn<FileId, bool, Meta>,
}

impl FileTable {
    fn new() -> Self {
        let generation = Generation::next();
        Self {
            generation,
            encoding: HistoryColumn::new(generation),
            excluded: HistoryColumn::new(generation),
        }
    }

    /// One row per table: all columns get their row here, so the index is the same.
    fn add_file(&mut self, encoding: Enc) -> FileId {
        let index = self.encoding.push_row(encoding, LOAD);
        let same = self.excluded.push_row(false, LOAD);
        assert_eq!(index, same);
        FileId(self.generation, index)
    }
}

#[test]
fn the_columns_of_one_table_share_the_id_of_a_row() {
    let mut table = FileTable::new();
    let first = table.add_file(Enc::Latin1);
    let second = table.add_file(Enc::Utf8);

    table.excluded.append(second, true, by_user(1));
    table.encoding.append(second, Enc::Windows1252, by_user(1));

    assert_eq!(table.excluded.current(first), Some(false));
    assert_eq!(table.excluded.current(second), Some(true));
    assert_eq!(table.encoding.current(second), Some(Enc::Windows1252));
}

// ── Generations ──────────────────────────────────────────────────────────────

#[test]
fn every_generation_is_new() {
    let numbers: Vec<Generation> = (0..1000).map(|_| Generation::next()).collect();
    let distinct: std::collections::HashSet<Generation> = numbers.iter().copied().collect();
    assert_eq!(distinct.len(), numbers.len());
}

#[test]
fn a_column_has_the_generation_it_was_made_with() {
    let generation = Generation::next();
    let column = Column::new(generation);
    assert_eq!(column.generation(), generation);
}

#[test]
fn an_id_of_another_generation_is_unknown_to_every_read() {
    let mut mine = new_column();
    let mut other = new_column();
    // same index 0 in both columns: only the generation tells them apart
    let _own = add(&mut mine, Enc::Latin1);
    let foreign = add(&mut other, Enc::Utf8);
    assert_eq!(foreign.idx(), 0);

    assert_eq!(mine.current(foreign), None);
    assert!(mine.versions(foreign).is_empty());
    assert_eq!(mine.version_count(foreign), 0);
    assert_eq!(mine.current_ref(foreign), None);
    assert_eq!(
        mine.at(VersionRef {
            id: foreign,
            version: 0
        }),
        None
    );
    assert_eq!(mine.latest_where(foreign, |_| true), None);
}

#[test]
fn an_id_of_another_generation_cannot_be_appended_to_and_nothing_changes() {
    let mut mine = new_column();
    let mut other = new_column();
    let own = add(&mut mine, Enc::Latin1);
    let foreign = add(&mut other, Enc::Utf8);

    assert_eq!(mine.append(foreign, Enc::Windows1252, by_user(1)), None);

    assert_eq!(mine.version_count(own), 1);
    assert_eq!(mine.current(own), Some(Enc::Latin1));
    assert_eq!(other.version_count(foreign), 1);
}

#[test]
fn after_a_rebuild_the_old_ids_do_not_match_the_new_rows() {
    // A rebuild makes a new column with a new generation. The old id has an index that
    // exists in the new column, but it must not find the new row.
    let mut before = new_column();
    let old = add(&mut before, Enc::Latin1);

    let mut after = new_column();
    let new = add(&mut after, Enc::Utf8);
    assert_eq!(old.idx(), new.idx());

    assert_eq!(after.current(old), None);
    assert_eq!(after.current(new), Some(Enc::Utf8));
    assert_ne!(old.generation(), new.generation());
}
