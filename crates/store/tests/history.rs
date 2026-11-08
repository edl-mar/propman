//! Specification of the history rules (see `store::history`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use store::history::{
    Column, EditId, EditKind, Journal, Policy, ReloadOutcome, Restorable, Row, Source, Status,
};

/// A stand-in for a value that the disk can detect and the user can force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Enc {
    Latin1,
    Win1252,
    Utf8,
    Utf8Bom,
}
use Enc::{Latin1, Utf8, Utf8Bom, Win1252};

const DISK: Source = Source::DISK;
const SHARED: Source = Source::SHARED;
const LOCAL: Source = Source::LOCAL;
const USER: Source = Source::USER;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn precedence() -> (Journal, Column<Enc>) {
    let mut journal = Journal::new();
    let column = journal.column::<Enc>(Policy::Precedence);
    (journal, column)
}

fn conflict() -> (Journal, Column<&'static str>) {
    let mut journal = Journal::new();
    let column = journal.column::<&'static str>(Policy::Conflict);
    (journal, column)
}

fn load<T: Copy + Eq>(j: &mut Journal, c: &mut Column<T>, value: T) -> Row {
    let mut edit = j.edit(EditKind::Load, DISK);
    let row = c.add_row(&mut edit, value);
    edit.commit();
    row
}

fn write<T: Copy + Eq>(
    j: &mut Journal,
    c: &mut Column<T>,
    row: Row,
    value: T,
    kind: EditKind,
    source: Source,
) -> EditId {
    let mut edit = j.edit(kind, source);
    edit.set(c, row, value);
    edit.commit()
}

fn user_sets<T: Copy + Eq>(j: &mut Journal, c: &mut Column<T>, row: Row, value: T) -> EditId {
    write(j, c, row, value, EditKind::User, USER)
}

fn reload<T: Copy + Eq>(j: &mut Journal, c: &mut Column<T>, row: Row, theirs: T) -> ReloadOutcome {
    let mut edit = j.edit(EditKind::Reload, DISK);
    let outcome = edit.reload(c, row, theirs);
    edit.commit();
    outcome
}

fn mark_saved<T: Copy + Eq>(j: &mut Journal, c: &mut Column<T>, row: Row, to: Source) -> bool {
    let mut edit = j.edit(EditKind::Save, to);
    let appended = edit.mark_saved(c, row);
    edit.commit();
    appended
}

fn restore<T: Copy + Eq>(j: &mut Journal, c: &mut Column<T>, row: Row, version: usize) -> bool {
    let mut edit = j.edit(EditKind::Restore, USER);
    let found = edit.restore(c, row, version);
    edit.commit();
    found
}

/// The history as (value, source) pairs.
fn trail<T: Copy + Eq>(j: &Journal, c: &Column<T>, row: Row) -> Vec<(T, Source)> {
    c.versions(j, row)
        .iter()
        .map(|v| (v.value, v.source))
        .collect()
}

// ── Layers and sources ───────────────────────────────────────────────────────

#[test]
fn layers_arrive_in_rank_order_and_the_last_one_wins() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    write(&mut j, &mut c, row, Win1252, EditKind::SettingsRead, SHARED);
    write(&mut j, &mut c, row, Utf8, EditKind::SettingsRead, LOCAL);
    user_sets(&mut j, &mut c, row, Utf8Bom);

    assert_eq!(
        trail(&j, &c, row),
        [
            (Latin1, DISK),
            (Win1252, SHARED),
            (Utf8, LOCAL),
            (Utf8Bom, USER)
        ]
    );
    assert_eq!(c.current(row), Some(Utf8Bom));
}

#[test]
fn every_version_knows_the_kind_and_source_of_its_edit() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    write(&mut j, &mut c, row, Win1252, EditKind::SettingsRead, SHARED);
    user_sets(&mut j, &mut c, row, Utf8);
    reload(&mut j, &mut c, row, Utf8Bom);
    mark_saved(&mut j, &mut c, row, LOCAL);
    restore(&mut j, &mut c, row, 0);

    let kinds: Vec<(EditKind, Source)> = c
        .versions(&j, row)
        .iter()
        .map(|v| (v.kind, v.source))
        .collect();
    assert_eq!(
        kinds,
        [
            (EditKind::Load, DISK),
            (EditKind::SettingsRead, SHARED),
            (EditKind::User, USER),
            (EditKind::Reload, DISK),
            (EditKind::Reload, USER), // the user's value, appended again by the reload
            (EditKind::Save, LOCAL),
            (EditKind::Restore, USER),
        ]
    );
}

#[test]
fn the_base_is_the_latest_version_of_a_source_and_falls_back_to_version_0() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    write(&mut j, &mut c, row, Win1252, EditKind::SettingsRead, SHARED);
    user_sets(&mut j, &mut c, row, Utf8);

    assert_eq!(c.base(&j, row, DISK), Some(Latin1));
    assert_eq!(c.base(&j, row, SHARED), Some(Win1252));
    assert_eq!(c.base(&j, row, LOCAL), Some(Latin1)); // no version from LOCAL: version 0
    assert_eq!(c.base(&j, row, USER), Some(Utf8));

    write(&mut j, &mut c, row, Utf8Bom, EditKind::SettingsRead, SHARED);
    assert_eq!(c.base(&j, row, SHARED), Some(Utf8Bom));
}

#[test]
fn a_value_is_dirty_for_a_source_if_it_differs_from_the_base_of_that_source() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    write(&mut j, &mut c, row, Utf8, EditKind::SettingsRead, LOCAL);
    assert!(!c.is_dirty(&j, row, LOCAL));

    user_sets(&mut j, &mut c, row, Win1252);
    assert!(c.is_dirty(&j, row, LOCAL));
    assert!(c.is_dirty(&j, row, DISK));

    user_sets(&mut j, &mut c, row, Utf8); // back to what LOCAL holds
    assert!(!c.is_dirty(&j, row, LOCAL));
}

// ── Setting and restoring ────────────────────────────────────────────────────

#[test]
fn setting_the_current_value_appends_nothing() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    let mut edit = j.edit(EditKind::User, USER);
    assert!(!edit.set(&mut c, row, Latin1));
    edit.commit();
    assert_eq!(c.versions(&j, row).len(), 1);
}

#[test]
fn restoring_appends_the_old_value_again_and_removes_nothing() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    user_sets(&mut j, &mut c, row, Utf8);
    user_sets(&mut j, &mut c, row, Win1252);

    assert!(restore(&mut j, &mut c, row, 0));

    assert_eq!(
        trail(&j, &c, row),
        [
            (Latin1, DISK),
            (Utf8, USER),
            (Win1252, USER),
            (Latin1, USER)
        ]
    );
    assert_eq!(c.current(row), Some(Latin1));
}

#[test]
fn restoring_an_unknown_version_does_nothing() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    assert!(!restore(&mut j, &mut c, row, 99));
    assert_eq!(c.versions(&j, row).len(), 1);
}

// ── Saving: the marker ───────────────────────────────────────────────────────

#[test]
fn saving_appends_a_marker_so_the_base_catches_up() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    user_sets(&mut j, &mut c, row, Win1252);
    assert!(c.is_dirty(&j, row, LOCAL));

    assert!(mark_saved(&mut j, &mut c, row, LOCAL));

    let last = *c.versions(&j, row).last().unwrap();
    assert_eq!(
        (last.value, last.source, last.kind),
        (Win1252, LOCAL, EditKind::Save)
    );
    assert!(!c.is_dirty(&j, row, LOCAL));
    assert!(c.is_dirty(&j, row, SHARED)); // not saved there
    assert!(c.is_dirty(&j, row, DISK));
}

#[test]
fn saving_a_value_that_is_not_dirty_appends_nothing() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    assert!(!mark_saved(&mut j, &mut c, row, DISK));
    assert_eq!(c.versions(&j, row).len(), 1);
}

// ── Reload with precedence ───────────────────────────────────────────────────

#[test]
fn a_reload_that_finds_nothing_new_appends_nothing() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    assert_eq!(
        reload(&mut j, &mut c, row, Latin1),
        ReloadOutcome::Unchanged
    );
    assert_eq!(c.versions(&j, row).len(), 1);

    user_sets(&mut j, &mut c, row, Utf8);
    assert_eq!(
        reload(&mut j, &mut c, row, Latin1),
        ReloadOutcome::Unchanged
    );
    assert_eq!(c.versions(&j, row).len(), 2);
    assert_eq!(c.current(row), Some(Utf8));
}

#[test]
fn a_new_disk_value_is_taken_if_nothing_ranks_higher() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);

    assert_eq!(reload(&mut j, &mut c, row, Win1252), ReloadOutcome::Taken);

    assert_eq!(trail(&j, &c, row), [(Latin1, DISK), (Win1252, DISK)]);
    assert_eq!(c.current(row), Some(Win1252));
    assert_eq!(c.base(&j, row, DISK), Some(Win1252));
}

#[test]
fn a_reload_keeps_the_decision_of_the_user_and_moves_the_disk_base() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    user_sets(&mut j, &mut c, row, Utf8);

    assert_eq!(reload(&mut j, &mut c, row, Win1252), ReloadOutcome::Kept);

    assert_eq!(
        trail(&j, &c, row),
        [(Latin1, DISK), (Utf8, USER), (Win1252, DISK), (Utf8, USER)]
    );
    assert_eq!(c.current(row), Some(Utf8));
    assert_eq!(c.base(&j, row, DISK), Some(Win1252));
    assert_eq!(c.status(&j, row), Status::Clean);

    // the two versions of the reload belong to one edit
    let versions = c.versions(&j, row);
    assert_eq!(versions[2].edit, versions[3].edit);
}

#[test]
fn only_the_highest_ranked_source_is_appended_again() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    write(&mut j, &mut c, row, Win1252, EditKind::SettingsRead, SHARED);
    write(&mut j, &mut c, row, Utf8, EditKind::SettingsRead, LOCAL);

    assert_eq!(reload(&mut j, &mut c, row, Utf8Bom), ReloadOutcome::Kept);

    assert_eq!(
        trail(&j, &c, row),
        [
            (Latin1, DISK),
            (Win1252, SHARED),
            (Utf8, LOCAL),
            (Utf8Bom, DISK),
            (Utf8, LOCAL),
        ]
    );
    assert_eq!(c.current(row), Some(Utf8));
}

#[test]
fn when_the_disk_agrees_with_the_current_value_only_the_disk_version_is_appended() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    user_sets(&mut j, &mut c, row, Utf8);

    assert_eq!(reload(&mut j, &mut c, row, Utf8), ReloadOutcome::Taken);

    assert_eq!(
        trail(&j, &c, row),
        [(Latin1, DISK), (Utf8, USER), (Utf8, DISK)]
    );
    assert_eq!(c.base(&j, row, DISK), Some(Utf8));
}

// ── Reload with conflict ─────────────────────────────────────────────────────

#[test]
fn a_one_sided_external_change_is_taken_without_conflict() {
    let (mut j, mut c) = conflict();
    let row = load(&mut j, &mut c, "a");

    assert_eq!(reload(&mut j, &mut c, row, "b"), ReloadOutcome::Taken);

    assert_eq!(c.current(row), Some("b"));
    assert_eq!(c.status(&j, row), Status::Clean);
}

#[test]
fn a_one_sided_local_change_survives_a_reload_that_finds_the_disk_unchanged() {
    let (mut j, mut c) = conflict();
    let row = load(&mut j, &mut c, "a");
    user_sets(&mut j, &mut c, row, "b");

    assert_eq!(reload(&mut j, &mut c, row, "a"), ReloadOutcome::Unchanged);

    assert_eq!(c.current(row), Some("b"));
    assert_eq!(c.status(&j, row), Status::Clean);
}

#[test]
fn both_sides_changing_is_a_conflict_and_nothing_is_lost() {
    let (mut j, mut c) = conflict();
    let row = load(&mut j, &mut c, "a");
    user_sets(&mut j, &mut c, row, "b");

    assert_eq!(reload(&mut j, &mut c, row, "c"), ReloadOutcome::Conflict);

    assert_eq!(trail(&j, &c, row), [("a", DISK), ("b", USER), ("c", DISK)]);
    assert_eq!(c.current(row), Some("c"));
    assert_eq!(c.status(&j, row), Status::Conflict);
    assert_eq!(c.base(&j, row, DISK), Some("c"));
    // reading again does not change anything
    assert_eq!(c.status(&j, row), Status::Conflict);
}

#[test]
fn both_sides_changing_to_the_same_value_is_no_conflict() {
    let (mut j, mut c) = conflict();
    let row = load(&mut j, &mut c, "a");
    user_sets(&mut j, &mut c, row, "b");

    assert_eq!(reload(&mut j, &mut c, row, "b"), ReloadOutcome::Taken);

    assert_eq!(c.status(&j, row), Status::Clean);
}

#[test]
fn a_conflict_is_resolved_by_restoring_our_version() {
    let (mut j, mut c) = conflict();
    let row = load(&mut j, &mut c, "a");
    user_sets(&mut j, &mut c, row, "b");
    reload(&mut j, &mut c, row, "c");

    assert!(restore(&mut j, &mut c, row, 1)); // ours

    assert_eq!(c.current(row), Some("b"));
    assert_eq!(c.status(&j, row), Status::Clean);
    assert_eq!(c.versions(&j, row).len(), 4);
}

#[test]
fn a_conflict_is_resolved_by_accepting_theirs() {
    let (mut j, mut c) = conflict();
    let row = load(&mut j, &mut c, "a");
    user_sets(&mut j, &mut c, row, "b");
    reload(&mut j, &mut c, row, "c");

    assert!(restore(&mut j, &mut c, row, 2)); // theirs

    assert_eq!(c.current(row), Some("c"));
    assert_eq!(c.status(&j, row), Status::Clean);
}

#[test]
fn a_second_external_change_does_not_hide_an_unresolved_conflict() {
    let (mut j, mut c) = conflict();
    let row = load(&mut j, &mut c, "a");
    user_sets(&mut j, &mut c, row, "b");
    reload(&mut j, &mut c, row, "c");

    assert_eq!(reload(&mut j, &mut c, row, "d"), ReloadOutcome::Conflict);
    assert_eq!(c.status(&j, row), Status::Conflict);

    assert!(restore(&mut j, &mut c, row, 1)); // keep ours
    assert_eq!(c.current(row), Some("b"));
    assert_eq!(c.status(&j, row), Status::Clean);
}

#[test]
fn a_precedence_column_never_reports_a_conflict() {
    let (mut j, mut c) = precedence();
    let row = load(&mut j, &mut c, Latin1);
    user_sets(&mut j, &mut c, row, Utf8);
    reload(&mut j, &mut c, row, Win1252);
    assert_eq!(c.status(&j, row), Status::Clean);
}

// ── Edits that bundle several writes ─────────────────────────────────────────

#[test]
fn an_edit_with_several_writes_is_undone_as_a_whole_across_columns() {
    let mut j = Journal::new();
    let mut enc = j.column::<Enc>(Policy::Precedence);
    let mut excluded = j.column::<bool>(Policy::Precedence);
    let enc_row = load(&mut j, &mut enc, Latin1);
    let flag_row = load(&mut j, &mut excluded, false);

    let target = {
        let mut edit = j.edit(EditKind::User, USER);
        edit.set(&mut enc, enc_row, Utf8);
        edit.set(&mut excluded, flag_row, true);
        edit.commit()
    };
    assert_eq!(enc.current(enc_row), Some(Utf8));
    assert_eq!(excluded.current(flag_row), Some(true));

    let mut undo = j.edit(EditKind::Restore, USER);
    undo.undo(
        target,
        &mut [
            &mut enc as &mut dyn Restorable,
            &mut excluded as &mut dyn Restorable,
        ],
    );
    undo.commit();

    assert_eq!(enc.current(enc_row), Some(Latin1));
    assert_eq!(excluded.current(flag_row), Some(false));
    // nothing was removed, and both restorations belong to one edit
    assert_eq!(enc.versions(&j, enc_row).len(), 3);
    assert_eq!(excluded.versions(&j, flag_row).len(), 3);
    assert_eq!(
        enc.versions(&j, enc_row)[2].edit,
        excluded.versions(&j, flag_row)[2].edit
    );
}

#[test]
fn an_edit_may_touch_several_rows_of_one_column() {
    let (mut j, mut c) = precedence();
    let first = load(&mut j, &mut c, Latin1);
    let second = load(&mut j, &mut c, Latin1);

    let target = {
        let mut edit = j.edit(EditKind::User, USER);
        edit.set(&mut c, first, Utf8);
        edit.set(&mut c, second, Win1252);
        edit.commit()
    };
    let mut undo = j.edit(EditKind::Restore, USER);
    undo.undo(target, &mut [&mut c as &mut dyn Restorable]);
    undo.commit();

    assert_eq!(c.current(first), Some(Latin1));
    assert_eq!(c.current(second), Some(Latin1));
}

// ── Nothing is ever removed ──────────────────────────────────────────────────

#[test]
fn the_history_only_ever_grows() {
    let (mut j, mut c) = conflict();
    let row = load(&mut j, &mut c, "a");
    let mut lengths = vec![c.versions(&j, row).len()];
    let mut check = |j: &Journal, c: &Column<&'static str>| {
        let len = c.versions(j, row).len();
        assert!(len >= *lengths.last().unwrap());
        lengths.push(len);
    };

    user_sets(&mut j, &mut c, row, "b");
    check(&j, &c);
    reload(&mut j, &mut c, row, "c");
    check(&j, &c);
    restore(&mut j, &mut c, row, 0);
    check(&j, &c);
    mark_saved(&mut j, &mut c, row, LOCAL);
    check(&j, &c);
    reload(&mut j, &mut c, row, "c");
    check(&j, &c);

    // the first versions are still the same ones
    let versions = trail(&j, &c, row);
    assert_eq!(versions[0], ("a", DISK));
    assert_eq!(versions[1], ("b", USER));
    assert_eq!(versions[2], ("c", DISK));
}
