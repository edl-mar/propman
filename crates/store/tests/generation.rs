//! Specification of the generation of ids in the history (see `store::history`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use store::history::{Column, EditKind, Journal, Policy, ReloadOutcome, Row, Source, Status};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Enc {
    Latin1,
    Utf8,
    Win1252,
}
use Enc::{Latin1, Utf8, Win1252};

/// A journal with one column and one row, as a store makes after loading.
fn world(initial: Enc) -> (Journal, Column<Enc>, Row) {
    let mut journal = Journal::new();
    let mut column = journal.column::<Enc>(Policy::Precedence);
    let mut edit = journal.edit(EditKind::Load, Source::DISK);
    let row = column.add_row(&mut edit, initial);
    edit.commit();
    (journal, column, row)
}

#[test]
fn every_journal_has_its_own_generation() {
    let first = Journal::new();
    let second = Journal::new();
    assert_ne!(first.generation(), second.generation());
}

#[test]
fn a_row_from_another_journal_is_unknown_to_every_read() {
    let (_, _, foreign) = world(Latin1);
    let (journal, column, own) = world(Utf8); // the same index 0 as the foreign row

    assert_eq!(column.current(foreign), None);
    assert!(column.versions(&journal, foreign).is_empty());
    assert_eq!(column.base(&journal, foreign, Source::DISK), None);
    assert!(!column.is_dirty(&journal, foreign, Source::LOCAL));
    assert_eq!(column.status(&journal, foreign), Status::Clean);

    // and the row of this journal at the same index is untouched
    assert_eq!(column.current(own), Some(Utf8));
}

#[test]
fn a_row_from_another_journal_cannot_be_written_and_nothing_changes() {
    let (_, _, foreign) = world(Latin1);
    let (mut journal, mut column, own) = world(Utf8);

    let mut edit = journal.edit(EditKind::User, Source::USER);
    assert!(!edit.set(&mut column, foreign, Win1252));
    assert!(!edit.restore(&mut column, foreign, 0));
    assert!(!edit.mark_saved(&mut column, foreign));
    edit.commit();

    let mut reload = journal.edit(EditKind::Reload, Source::DISK);
    assert_eq!(
        reload.reload(&mut column, foreign, Win1252),
        ReloadOutcome::Unchanged
    );
    reload.commit();

    assert_eq!(column.versions(&journal, own).len(), 1);
    assert_eq!(column.current(own), Some(Utf8));
}

#[test]
fn after_a_rebuild_old_rows_are_unknown_and_new_rows_work() {
    let (_, _, old) = world(Latin1);

    // a rebuild is a new journal with new columns
    let (mut journal, mut column, new) = world(Utf8);
    assert_eq!(column.current(old), None);

    let mut edit = journal.edit(EditKind::User, Source::USER);
    assert!(edit.set(&mut column, new, Win1252));
    edit.commit();
    assert_eq!(column.current(new), Some(Win1252));
    assert_eq!(column.current(old), None);
}
