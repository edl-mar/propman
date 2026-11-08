//! Specification of the session: loading a folder into the store and saving back, including the
//! golden roundtrip through the store (see `app::Session`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use app::{FileId, SaveError, Session};
use common::{TempDir, copy_dir, fixtures, snapshot};
use disk::encoding::{decode, encode};
use types::{Confidence, Encoding};

/// A copy of the fixtures that the test may change.
fn project() -> TempDir {
    let dir = TempDir::new();
    copy_dir(&fixtures(), dir.path());
    dir
}

fn open(dir: &TempDir) -> Session {
    let (session, problems) = Session::open(dir.path());
    assert!(problems.is_empty(), "{problems:?}");
    session
}

fn file(session: &Session, dir: &TempDir, relative: &str) -> FileId {
    session
        .store()
        .file_by_path(&dir.path().join(relative))
        .unwrap_or_else(|| panic!("{relative} was not loaded"))
}

// ── Loading ──────────────────────────────────────────────────────────────────

#[test]
fn open_loads_every_properties_file_below_the_folder() {
    let dir = project();
    let session = open(&dir);
    // 12 in project/ and 5 in edge/encodings/
    assert_eq!(session.store().files().len(), 17);
}

#[test]
fn files_are_grouped_into_bundles_by_folder_and_name() {
    let dir = project();
    let session = open(&dir);
    let store = session.store();

    let names: BTreeSet<(String, String)> = store
        .bundles()
        .iter()
        .map(|&b| {
            let folder = store.bundle_folder(b).unwrap();
            let relative = folder.strip_prefix(dir.path()).unwrap();
            (
                relative.display().to_string().replace('\\', "/"),
                store.bundle_name(b).unwrap().to_owned(),
            )
        })
        .collect();
    let expected: BTreeSet<(String, String)> = [
        ("project", "messages"),
        ("project", "stripping"),
        ("project", "testbundle"),
        ("project/sub_folder", "translations"),
        ("edge/encodings", "messages"),
    ]
    .iter()
    .map(|&(folder, name)| (folder.to_owned(), name.to_owned()))
    .collect();
    assert_eq!(names, expected);
}

#[test]
fn the_locale_and_confidence_of_a_file_come_from_its_name() {
    let dir = project();
    let session = open(&dir);
    let store = session.store();

    let german = file(&session, &dir, "project/messages_de.properties");
    let assignment = store.file_assignment(german).unwrap();
    assert_eq!(store.text(assignment.locale.unwrap()), Some("de"));
    assert_eq!(assignment.confidence, Confidence::Certain);
    assert_eq!(store.bundle_name(assignment.bundle), Some("messages"));

    let default = file(&session, &dir, "project/messages.properties");
    assert_eq!(store.file_assignment(default).unwrap().locale, None);
}

#[test]
fn the_encoding_of_every_file_is_detected() {
    let dir = project();
    let session = open(&dir);
    let encoding = |relative: &str| {
        session
            .store()
            .file_encoding(file(&session, &dir, relative))
            .unwrap()
    };
    assert_eq!(
        encoding("edge/encodings/messages.properties"),
        Encoding::Utf8
    );
    assert_eq!(
        encoding("edge/encodings/messages_de.properties"),
        Encoding::Utf8Bom
    );
    assert_eq!(
        encoding("edge/encodings/messages_fr.properties"),
        Encoding::Latin1
    );
    assert_eq!(
        encoding("edge/encodings/messages_it.properties"),
        Encoding::Windows1252
    );
}

#[test]
fn the_store_holds_the_bytes_and_lines_that_are_on_the_disk() {
    let dir = project();
    let session = open(&dir);
    let store = session.store();

    for (relative, bytes) in snapshot(dir.path()) {
        let id = file(&session, &dir, &relative);
        assert_eq!(store.file_bytes(id), Some(&bytes[..]), "{relative}");

        let text = decode(&bytes, None).unwrap().text;
        let lines: String = store
            .file_lines(id)
            .iter()
            .map(|l| format!("{}{}", l.content, l.ending.as_str()))
            .collect();
        assert_eq!(lines, text, "{relative}");
    }
}

#[test]
fn a_missing_folder_is_a_problem_and_not_a_crash() {
    let dir = TempDir::new();
    let (session, problems) = Session::open(&dir.path().join("does_not_exist"));
    assert!(session.store().files().is_empty());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(!problems[0].message.is_empty());
}

#[test]
fn an_empty_folder_gives_an_empty_session_without_problems() {
    let dir = TempDir::new();
    let (session, problems) = Session::open(dir.path());
    assert!(session.store().files().is_empty());
    assert!(problems.is_empty());
    assert_eq!(session.root(), dir.path());
}

// ── The golden roundtrip through the store ───────────────────────────────────

#[test]
fn nothing_is_dirty_after_loading() {
    let dir = project();
    let session = open(&dir);
    assert!(session.dirty_files().is_empty());
}

#[test]
fn saving_every_file_unchanged_gives_the_same_bytes() {
    let dir = project();
    let before = snapshot(dir.path());
    let mut session = open(&dir);

    for id in session.store().files() {
        session.save_file(id).unwrap();
    }

    assert_eq!(snapshot(dir.path()), before);
    assert!(session.dirty_files().is_empty());
}

#[test]
fn saving_unchanged_files_does_not_change_what_the_store_holds() {
    let dir = project();
    let mut session = open(&dir);
    let id = file(&session, &dir, "edge/encodings/messages_pl.properties");
    let lines_before: Vec<(String, _)> = session
        .store()
        .file_lines(id)
        .iter()
        .map(|l| (l.content.to_owned(), l.ending))
        .collect();

    session.save_file(id).unwrap();

    let lines_after: Vec<(String, _)> = session
        .store()
        .file_lines(id)
        .iter()
        .map(|l| (l.content.to_owned(), l.ending))
        .collect();
    assert_eq!(lines_after, lines_before);
    // the lines did not change, so there is no new snapshot
    assert_eq!(session.store().snapshots(id), 1);
}

// ── Changing the encoding and saving ─────────────────────────────────────────

#[test]
fn another_encoding_makes_a_file_dirty_and_saving_changes_only_that_file() {
    let dir = project();
    let before = snapshot(dir.path());
    let mut session = open(&dir);
    let id = file(&session, &dir, "edge/encodings/messages_fr.properties");

    assert!(session.set_encoding(id, Encoding::Utf8));
    assert_eq!(session.dirty_files(), [id]);

    session.save_file(id).unwrap();

    let after = snapshot(dir.path());
    for ((path, old), (path_after, new)) in before.iter().zip(&after) {
        assert_eq!(path, path_after);
        if path == "edge/encodings/messages_fr.properties" {
            // Latin-1 bytes read as characters, written as UTF-8
            let text: String = old.iter().map(|&b| char::from(b)).collect();
            assert_eq!(new, text.as_bytes());
        } else {
            assert_eq!(new, old, "{path}");
        }
    }
    assert!(session.dirty_files().is_empty());
    assert_eq!(session.store().file_encoding(id), Some(Encoding::Utf8));
    let on_disk = std::fs::read(dir.path().join("edge/encodings/messages_fr.properties")).unwrap();
    assert_eq!(session.store().file_bytes(id), Some(&on_disk[..]));
}

#[test]
fn a_choice_that_makes_no_difference_on_the_disk_is_not_dirty() {
    // pure ASCII: the same bytes in every encoding
    let dir = project();
    let mut session = open(&dir);
    let id = file(&session, &dir, "edge/encodings/messages.properties");

    assert!(session.set_encoding(id, Encoding::Latin1));

    assert_eq!(session.store().file_encoding(id), Some(Encoding::Latin1));
    assert!(session.dirty_files().is_empty());
}

#[test]
fn a_character_the_encoding_cannot_hold_is_saved_as_an_escape_and_read_back_as_such() {
    let dir = project();
    let mut session = open(&dir);
    let id = file(&session, &dir, "edge/encodings/messages_it.properties");
    let original = std::fs::read(dir.path().join("edge/encodings/messages_it.properties")).unwrap();

    session.set_encoding(id, Encoding::Latin1);
    session.save_file(id).unwrap();

    // what the disk now holds is what `encode` makes of the text, with escapes
    let text = decode(&original, None).unwrap().text;
    let expected = encode(&text, Encoding::Latin1);
    let on_disk = std::fs::read(dir.path().join("edge/encodings/messages_it.properties")).unwrap();
    assert_eq!(on_disk, expected);
    assert!(on_disk.windows(6).any(|w| w == b"\\u20AC"));

    // the store shows what is on the disk, as a new snapshot; the old one is kept
    let store = session.store();
    assert_eq!(store.snapshots(id), 2);
    assert!(
        store
            .file_lines(id)
            .iter()
            .any(|l| l.content.contains("\\u20AC"))
    );
    assert!(
        store
            .file_lines_in_snapshot(id, 0)
            .iter()
            .any(|l| l.content.contains('\u{20ac}'))
    );
    assert_eq!(store.file_bytes(id), Some(&on_disk[..]));
    assert!(session.dirty_files().is_empty());
}

#[test]
fn an_encoding_cannot_be_chosen_for_an_unknown_file() {
    let dir = project();
    let mut session = open(&dir);
    let other_dir = project();
    let other = open(&other_dir);
    let foreign = other.store().files()[0];
    assert!(!session.set_encoding(foreign, Encoding::Utf8));
}

// ── Saving is careful ────────────────────────────────────────────────────────

#[test]
fn a_file_changed_on_the_disk_is_not_overwritten() {
    let dir = project();
    let mut session = open(&dir);
    let path = "project/messages_de.properties";
    let id = file(&session, &dir, path);
    std::fs::write(dir.path().join(path), b"changed by someone else\n").unwrap();

    let result = session.save_file(id);

    assert!(matches!(result, Err(SaveError::Changed)), "{result:?}");
    assert_eq!(
        std::fs::read(dir.path().join(path)).unwrap(),
        b"changed by someone else\n"
    );
}

#[test]
fn a_file_deleted_on_the_disk_is_not_recreated() {
    let dir = project();
    let mut session = open(&dir);
    let path = "project/messages_de.properties";
    let id = file(&session, &dir, path);
    std::fs::remove_file(dir.path().join(path)).unwrap();

    let result = session.save_file(id);

    assert!(matches!(result, Err(SaveError::Deleted)), "{result:?}");
    assert!(!Path::new(&dir.path().join(path)).exists());
}

#[test]
fn a_file_of_another_session_cannot_be_saved() {
    let dir = project();
    let mut session = open(&dir);
    let other_dir = project();
    let other = open(&other_dir);
    let foreign = other.store().files()[0];

    let result = session.save_file(foreign);

    assert!(matches!(result, Err(SaveError::UnknownFile)), "{result:?}");
    assert_eq!(snapshot(dir.path()), snapshot(&fixtures()));
}

#[test]
fn a_touch_without_a_change_does_not_block_saving() {
    let dir = project();
    let mut session = open(&dir);
    let path = "project/messages_de.properties";
    let id = file(&session, &dir, path);
    // same content, new modification time
    let content = std::fs::read(dir.path().join(path)).unwrap();
    std::fs::write(dir.path().join(path), &content).unwrap();

    assert!(session.save_file(id).is_ok());
    assert_eq!(std::fs::read(dir.path().join(path)).unwrap(), content);
}

// ── Excluding a bundle ───────────────────────────────────────────────────────

#[test]
fn a_bundle_can_be_excluded_and_included_again() {
    let dir = project();
    let mut session = open(&dir);
    let bundle = session.store().bundles()[0];
    assert_eq!(session.store().bundle_excluded(bundle), Some(false));

    assert!(session.exclude_bundle(bundle, true));
    assert_eq!(session.store().bundle_excluded(bundle), Some(true));
    assert!(!session.exclude_bundle(bundle, true));
    assert!(session.exclude_bundle(bundle, false));
    assert_eq!(session.store().bundle_excluded(bundle), Some(false));
}

#[test]
fn excluding_a_bundle_does_not_touch_the_disk_or_make_files_dirty() {
    let dir = project();
    let before = snapshot(dir.path());
    let mut session = open(&dir);
    let bundle = session.store().bundles()[0];

    session.exclude_bundle(bundle, true);

    assert!(session.dirty_files().is_empty());
    assert_eq!(snapshot(dir.path()), before);
}

#[test]
fn a_bundle_of_another_session_cannot_be_excluded() {
    let dir = project();
    let mut session = open(&dir);
    let other_dir = project();
    let other = open(&other_dir);
    let foreign = other.store().bundles()[0];

    assert!(!session.exclude_bundle(foreign, true));
    for bundle in session.store().bundles() {
        assert_eq!(session.store().bundle_excluded(bundle), Some(false));
    }
}
