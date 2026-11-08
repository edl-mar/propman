//! Specification of the first store: the raw unit with bundles, files and lines, the decisions of
//! the user and the ids of one store (see `store::Store`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use store::history::{EditId, EditKind, Source};
use store::{
    BundleAssignment, BundleId, FileId, FileInput, FileReload, LineInput, Store, StoreEdit,
};
use types::{Confidence, Encoding, Fingerprint, LineEnding};

use LineEnding::{Cr, CrLf, Lf, None as NoEnd};

const DISK: Source = Source::DISK;
const SHARED: Source = Source::SHARED;
const LOCAL: Source = Source::LOCAL;
const USER: Source = Source::USER;

// ── Helpers ──────────────────────────────────────────────────────────────────

/// A file as `app` would hand it to the store after reading and classifying it.
#[derive(Clone)]
struct Spec {
    path: &'static str,
    bundle: &'static str,
    locale: Option<&'static str>,
    lines: Vec<(&'static str, LineEnding)>,
    bytes: Vec<u8>,
    encoding: Encoding,
    confidence: Confidence,
}

impl Spec {
    fn new(path: &'static str, bundle: &'static str, locale: Option<&'static str>) -> Self {
        Spec {
            path,
            bundle,
            locale,
            lines: vec![("a=1", Lf)],
            bytes: b"a=1\n".to_vec(),
            encoding: Encoding::Latin1,
            confidence: Confidence::Certain,
        }
    }

    fn lines(mut self, lines: &[(&'static str, LineEnding)]) -> Self {
        self.lines = lines.to_vec();
        self
    }

    fn bytes(mut self, bytes: &[u8]) -> Self {
        self.bytes = bytes.to_vec();
        self
    }

    fn encoding(mut self, encoding: Encoding) -> Self {
        self.encoding = encoding;
        self
    }

    fn confidence(mut self, confidence: Confidence) -> Self {
        self.confidence = confidence;
        self
    }
}

fn fingerprint(bytes: &[u8]) -> Fingerprint {
    Fingerprint {
        len: bytes.len() as u64,
        modified: None,
        hash: bytes
            .iter()
            .fold(7u64, |h, &b| h.wrapping_mul(31).wrapping_add(u64::from(b))),
    }
}

fn edit_as<R>(
    store: &mut Store,
    kind: EditKind,
    source: Source,
    f: impl FnOnce(&mut StoreEdit<'_>) -> R,
) -> (R, EditId) {
    let mut edit = store.edit(kind, source);
    let result = f(&mut edit);
    let id = edit.commit();
    (result, id)
}

fn user<R>(store: &mut Store, f: impl FnOnce(&mut StoreEdit<'_>) -> R) -> R {
    edit_as(store, EditKind::User, USER, f).0
}

fn load(store: &mut Store, spec: &Spec) -> Option<FileId> {
    let lines: Vec<LineInput<'_>> = spec
        .lines
        .iter()
        .map(|&(content, ending)| LineInput { content, ending })
        .collect();
    let input = FileInput {
        path: Path::new(spec.path),
        bytes: &spec.bytes,
        encoding: spec.encoding,
        fingerprint: fingerprint(&spec.bytes),
        lines: &lines,
        bundle: spec.bundle,
        locale: spec.locale,
        confidence: spec.confidence,
    };
    edit_as(store, EditKind::Load, DISK, |e| e.add_file(input)).0
}

fn load_ok(store: &mut Store, spec: &Spec) -> FileId {
    load(store, spec).unwrap()
}

fn bundle_of(store: &Store, file: FileId) -> BundleId {
    store.file_assignment(file).unwrap().bundle
}

fn locale_of(store: &Store, file: FileId) -> Option<String> {
    let assignment = store.file_assignment(file).unwrap();
    assignment
        .locale
        .map(|id| store.text(id).unwrap().to_owned())
}

// ── Loading and reading ──────────────────────────────────────────────────────

#[test]
fn a_new_store_has_no_files_and_no_bundles() {
    let store = Store::new();
    assert!(store.files().is_empty());
    assert!(store.bundles().is_empty());
}

#[test]
fn adding_a_file_stores_path_bytes_encoding_fingerprint_and_lines() {
    let mut store = Store::new();
    let spec = Spec::new("proj/messages_de.properties", "messages", Some("de"))
        .bytes(b"a=1\r\nb=2")
        .lines(&[("a=1", CrLf), ("b=2", NoEnd)])
        .encoding(Encoding::Utf8);
    let file = load_ok(&mut store, &spec);

    assert_eq!(store.files(), [file]);
    assert_eq!(
        store.file_path(file),
        Some(Path::new("proj/messages_de.properties"))
    );
    assert_eq!(store.file_bytes(file), Some(&b"a=1\r\nb=2"[..]));
    assert_eq!(store.file_encoding(file), Some(Encoding::Utf8));
    assert_eq!(
        store.file_fingerprint(file),
        Some(fingerprint(b"a=1\r\nb=2"))
    );
    assert_eq!(
        store.file_by_path(Path::new("proj/messages_de.properties")),
        Some(file)
    );
    assert_eq!(store.file_by_path(Path::new("proj/other.properties")), None);
}

#[test]
fn the_original_bytes_are_kept_exactly() {
    let mut store = Store::new();
    let bytes = [0xef, 0xbb, 0xbf, b'a', 0x00, 0xff, b'\r'];
    let file = load_ok(
        &mut store,
        &Spec::new("p/x.properties", "x", None)
            .bytes(&bytes)
            .lines(&[]),
    );
    assert_eq!(store.file_bytes(file), Some(&bytes[..]));
}

#[test]
fn lines_keep_their_order_and_endings() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/x.properties", "x", None).lines(&[
            ("# c", Lf),
            ("a=1", CrLf),
            ("", Cr),
            ("b=2", NoEnd),
        ]),
    );

    let lines = store.file_lines(file);
    let got: Vec<(&str, LineEnding)> = lines.iter().map(|l| (l.content, l.ending)).collect();
    assert_eq!(got, [("# c", Lf), ("a=1", CrLf), ("", Cr), ("b=2", NoEnd)]);
    let text: String = lines
        .iter()
        .map(|l| format!("{}{}", l.content, l.ending.as_str()))
        .collect();
    assert_eq!(text, "# c\na=1\r\n\rb=2");
}

#[test]
fn an_empty_file_has_no_lines() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/x.properties", "x", None).bytes(b"").lines(&[]),
    );
    assert!(store.file_lines(file).is_empty());
    assert_eq!(store.files(), [file]);
}

#[test]
fn the_lines_of_different_files_stay_apart() {
    let mut store = Store::new();
    let first = load_ok(
        &mut store,
        &Spec::new("p/a.properties", "a", None).lines(&[("x=1", Lf), ("y=2", Lf)]),
    );
    let second = load_ok(
        &mut store,
        &Spec::new("p/b.properties", "b", None).lines(&[("z=3", Lf)]),
    );

    let contents = |file| -> Vec<String> {
        store
            .file_lines(file)
            .iter()
            .map(|l| l.content.to_owned())
            .collect()
    };
    assert_eq!(contents(first), ["x=1", "y=2"]);
    assert_eq!(contents(second), ["z=3"]);
}

#[test]
fn files_are_listed_in_the_order_they_were_loaded() {
    let mut store = Store::new();
    let first = load_ok(&mut store, &Spec::new("p/c.properties", "c", None));
    let second = load_ok(&mut store, &Spec::new("p/a.properties", "a", None));
    let third = load_ok(&mut store, &Spec::new("p/b.properties", "b", None));
    assert_eq!(store.files(), [first, second, third]);
}

#[test]
fn a_path_identifies_a_file_and_is_added_once() {
    let mut store = Store::new();
    let first = load_ok(&mut store, &Spec::new("p/m.properties", "m", None));

    let again = load(
        &mut store,
        &Spec::new("p/m.properties", "other", Some("fr")).lines(&[("z=9", Lf)]),
    );

    assert_eq!(again, None);
    assert_eq!(store.files(), [first]);
    assert_eq!(store.file_lines(first).len(), 1);
    // the refused file did not leave a bundle behind
    assert_eq!(store.bundles().len(), 1);
}

// ── Bundles and assignments ──────────────────────────────────────────────────

#[test]
fn files_of_the_same_folder_and_name_share_a_bundle() {
    let mut store = Store::new();
    let default = load_ok(
        &mut store,
        &Spec::new("p/messages.properties", "messages", None),
    );
    let german = load_ok(
        &mut store,
        &Spec::new("p/messages_de.properties", "messages", Some("de")),
    );

    assert_eq!(store.bundles().len(), 1);
    assert_eq!(bundle_of(&store, default), bundle_of(&store, german));
    assert_eq!(
        store.files_of(bundle_of(&store, default)),
        [default, german]
    );
}

#[test]
fn the_same_name_in_another_folder_is_another_bundle() {
    let mut store = Store::new();
    let first = load_ok(
        &mut store,
        &Spec::new("p/messages_de.properties", "messages", Some("de")),
    );
    let second = load_ok(
        &mut store,
        &Spec::new("q/messages_de.properties", "messages", Some("de")),
    );

    assert_eq!(store.bundles().len(), 2);
    assert_ne!(bundle_of(&store, first), bundle_of(&store, second));
}

#[test]
fn a_bundle_has_the_folder_of_its_file_a_name_and_is_not_excluded() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("proj/sub/messages_de.properties", "messages", Some("de")),
    );
    let bundle = bundle_of(&store, file);

    assert_eq!(store.bundle_folder(bundle), Some(Path::new("proj/sub")));
    assert_eq!(store.bundle_name(bundle), Some("messages"));
    assert_eq!(store.bundle_excluded(bundle), Some(false));
}

#[test]
fn a_file_is_assigned_with_its_locale_and_confidence() {
    let mut store = Store::new();
    let german = load_ok(
        &mut store,
        &Spec::new("p/m_de.properties", "m", Some("de")).confidence(Confidence::Uncertain),
    );
    let default = load_ok(&mut store, &Spec::new("p/m.properties", "m", None));

    let assignment = store.file_assignment(german).unwrap();
    assert_eq!(store.bundle_name(assignment.bundle), Some("m"));
    assert_eq!(locale_of(&store, german), Some("de".to_owned()));
    assert_eq!(assignment.confidence, Confidence::Uncertain);
    assert_eq!(locale_of(&store, default), None);
}

#[test]
fn equal_locales_share_one_id_in_the_text_pool() {
    let mut store = Store::new();
    let first = load_ok(&mut store, &Spec::new("p/a_de.properties", "a", Some("de")));
    let second = load_ok(&mut store, &Spec::new("q/b_de.properties", "b", Some("de")));

    let first_locale = store.file_assignment(first).unwrap().locale;
    let second_locale = store.file_assignment(second).unwrap().locale;
    assert!(first_locale.is_some());
    assert_eq!(first_locale, second_locale);
}

#[test]
fn what_is_loaded_is_version_0_from_the_disk() {
    let mut store = Store::new();
    let file = load_ok(&mut store, &Spec::new("p/m.properties", "m", None));

    let encoding = store.encoding_versions(file);
    assert_eq!(encoding.len(), 1);
    assert_eq!(
        (encoding[0].value, encoding[0].source, encoding[0].kind),
        (Encoding::Latin1, DISK, EditKind::Load)
    );
    let assignment = store.assignment_versions(file);
    assert_eq!(assignment.len(), 1);
    assert_eq!(
        (assignment[0].source, assignment[0].kind),
        (DISK, EditKind::Load)
    );
    assert_eq!(Some(assignment[0].value), store.file_assignment(file));

    let excluded = store.excluded_versions(bundle_of(&store, file));
    assert_eq!(excluded.len(), 1);
    assert!(!excluded[0].value);
}

#[test]
fn asking_for_a_bundle_finds_the_existing_one_or_creates_a_new_one() {
    let mut store = Store::new();
    let file = load_ok(&mut store, &Spec::new("p/m.properties", "m", None));
    let existing = bundle_of(&store, file);

    assert_eq!(
        user(&mut store, |e| e.bundle(Path::new("p"), "m")),
        existing
    );
    assert_eq!(store.bundles().len(), 1);

    let created = user(&mut store, |e| e.bundle(Path::new("p"), "other"));
    assert_ne!(created, existing);
    assert_eq!(store.bundles().len(), 2);
    assert_eq!(store.bundle_name(created), Some("other"));
    assert!(store.files_of(created).is_empty());
}

// ── Decisions of the user ────────────────────────────────────────────────────

#[test]
fn setting_the_encoding_appends_a_user_version_and_keeps_the_detected_one() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/m.properties", "m", None).encoding(Encoding::Latin1),
    );

    assert!(user(&mut store, |e| e.set_encoding(file, Encoding::Utf8)));

    let trail: Vec<_> = store
        .encoding_versions(file)
        .iter()
        .map(|v| (v.value, v.source, v.kind))
        .collect();
    assert_eq!(
        trail,
        [
            (Encoding::Latin1, DISK, EditKind::Load),
            (Encoding::Utf8, USER, EditKind::User)
        ]
    );
    assert_eq!(store.file_encoding(file), Some(Encoding::Utf8));
    assert_eq!(store.encoding_base(file, DISK), Some(Encoding::Latin1));
}

#[test]
fn setting_the_current_encoding_changes_nothing() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/m.properties", "m", None).encoding(Encoding::Latin1),
    );
    assert!(!user(&mut store, |e| e.set_encoding(file, Encoding::Latin1)));
    assert_eq!(store.encoding_versions(file).len(), 1);
}

#[test]
fn a_file_can_be_moved_to_another_bundle_and_locale_and_back() {
    let mut store = Store::new();
    let file = load_ok(&mut store, &Spec::new("p/m_de.properties", "m", Some("de")));
    let original = store.file_assignment(file).unwrap();
    let other = user(&mut store, |e| e.bundle(Path::new("p"), "other"));

    let (changed, edit) = edit_as(&mut store, EditKind::User, USER, |e| {
        e.set_assignment(file, other, Some("fr"), Confidence::Uncertain)
    });
    assert!(changed);

    let moved = store.file_assignment(file).unwrap();
    assert_eq!(moved.bundle, other);
    assert_eq!(store.text(moved.locale.unwrap()), Some("fr"));
    assert_eq!(moved.confidence, Confidence::Uncertain);
    assert_eq!(store.files_of(other), [file]);
    assert!(store.files_of(original.bundle).is_empty());

    user(&mut store, |e| e.undo(edit));

    assert_eq!(store.file_assignment(file), Some(original));
    assert_eq!(store.files_of(original.bundle), [file]);
    assert!(store.files_of(other).is_empty());
}

#[test]
fn a_confirmation_changes_only_the_confidence() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/m_de.properties", "m", Some("de")).confidence(Confidence::Uncertain),
    );
    let before = store.file_assignment(file).unwrap();

    assert!(user(&mut store, |e| {
        e.set_assignment(file, before.bundle, Some("de"), Confidence::Certain)
    }));

    let after = store.file_assignment(file).unwrap();
    assert_eq!(after.bundle, before.bundle);
    assert_eq!(after.locale, before.locale);
    assert_eq!(after.confidence, Confidence::Certain);
    assert_eq!(store.assignment_versions(file).len(), 2);
    assert!(store.file_is_dirty(file, LOCAL));
}

#[test]
fn assigning_the_current_assignment_again_changes_nothing() {
    let mut store = Store::new();
    let file = load_ok(&mut store, &Spec::new("p/m_de.properties", "m", Some("de")));
    let current = store.file_assignment(file).unwrap();

    assert!(!user(&mut store, |e| {
        e.set_assignment(file, current.bundle, Some("de"), current.confidence)
    }));
    assert_eq!(store.assignment_versions(file).len(), 1);
}

#[test]
fn a_bundle_of_another_store_is_refused() {
    let mut store = Store::new();
    let file = load_ok(&mut store, &Spec::new("p/m_de.properties", "m", Some("de")));
    let before = store.file_assignment(file).unwrap();

    let mut other = Store::new();
    let foreign = user(&mut other, |e| e.bundle(Path::new("p"), "x"));

    assert!(!user(&mut store, |e| {
        e.set_assignment(file, foreign, Some("de"), Confidence::Certain)
    }));
    assert_eq!(store.file_assignment(file), Some(before));
    assert_eq!(store.assignment_versions(file).len(), 1);
}

#[test]
fn a_bundle_can_be_excluded_and_included_again() {
    let mut store = Store::new();
    let file = load_ok(&mut store, &Spec::new("p/m.properties", "m", None));
    let bundle = bundle_of(&store, file);

    assert!(user(&mut store, |e| e.exclude_bundle(bundle, true)));
    assert_eq!(store.bundle_excluded(bundle), Some(true));
    assert!(!user(&mut store, |e| e.exclude_bundle(bundle, true)));
    assert!(user(&mut store, |e| e.exclude_bundle(bundle, false)));
    assert_eq!(store.bundle_excluded(bundle), Some(false));

    let values: Vec<bool> = store
        .excluded_versions(bundle)
        .iter()
        .map(|v| v.value)
        .collect();
    assert_eq!(values, [false, true, false]);
}

#[test]
fn excluding_a_bundle_leaves_its_files_alone() {
    let mut store = Store::new();
    let file = load_ok(&mut store, &Spec::new("p/m.properties", "m", None));
    let bundle = bundle_of(&store, file);
    let assignment = store.file_assignment(file);

    user(&mut store, |e| e.exclude_bundle(bundle, true));

    assert_eq!(store.files_of(bundle), [file]);
    assert_eq!(store.file_assignment(file), assignment);
    assert_eq!(store.file_lines(file).len(), 1);
}

#[test]
fn an_edit_with_several_changes_is_undone_as_a_whole() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/m_de.properties", "m", Some("de")).encoding(Encoding::Latin1),
    );
    let bundle = bundle_of(&store, file);
    let original = store.file_assignment(file).unwrap();
    let other = user(&mut store, |e| e.bundle(Path::new("p"), "other"));

    let (_, edit) = edit_as(&mut store, EditKind::User, USER, |e| {
        e.set_encoding(file, Encoding::Utf8);
        e.exclude_bundle(bundle, true);
        e.set_assignment(file, other, Some("fr"), Confidence::Uncertain);
    });
    assert_eq!(store.file_encoding(file), Some(Encoding::Utf8));

    edit_as(&mut store, EditKind::Restore, USER, |e| e.undo(edit));

    assert_eq!(store.file_encoding(file), Some(Encoding::Latin1));
    assert_eq!(store.bundle_excluded(bundle), Some(false));
    assert_eq!(store.file_assignment(file), Some(original));
    // nothing was removed, and the undo is one edit
    assert_eq!(store.encoding_versions(file).len(), 3);
    assert_eq!(store.excluded_versions(bundle).len(), 3);
    assert_eq!(
        store.encoding_versions(file)[2].edit,
        store.excluded_versions(bundle)[2].edit
    );
}

// ── Saving: dirty and the marker ─────────────────────────────────────────────

#[test]
fn a_changed_file_is_dirty_until_it_is_marked_as_saved() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/m.properties", "m", None).encoding(Encoding::Latin1),
    );
    assert!(!store.file_is_dirty(file, LOCAL));

    user(&mut store, |e| e.set_encoding(file, Encoding::Utf8));
    assert!(store.file_is_dirty(file, LOCAL));
    assert!(store.file_is_dirty(file, DISK));

    let (appended, _) = edit_as(&mut store, EditKind::Save, LOCAL, |e| {
        e.mark_file_saved(file)
    });
    assert!(appended);

    let last = *store.encoding_versions(file).last().unwrap();
    assert_eq!(
        (last.value, last.source, last.kind),
        (Encoding::Utf8, LOCAL, EditKind::Save)
    );
    assert!(!store.file_is_dirty(file, LOCAL));
    assert!(store.file_is_dirty(file, SHARED)); // not saved there
}

#[test]
fn marking_a_clean_file_as_saved_appends_nothing() {
    let mut store = Store::new();
    let file = load_ok(&mut store, &Spec::new("p/m.properties", "m", None));
    let (appended, _) = edit_as(&mut store, EditKind::Save, LOCAL, |e| {
        e.mark_file_saved(file)
    });
    assert!(!appended);
    assert_eq!(store.encoding_versions(file).len(), 1);
    assert_eq!(store.assignment_versions(file).len(), 1);
}

#[test]
fn a_bundle_is_dirty_until_its_excluded_flag_is_marked_as_saved() {
    let mut store = Store::new();
    let file = load_ok(&mut store, &Spec::new("p/m.properties", "m", None));
    let bundle = bundle_of(&store, file);

    user(&mut store, |e| e.exclude_bundle(bundle, true));
    assert!(store.bundle_is_dirty(bundle, LOCAL));

    let (appended, _) = edit_as(&mut store, EditKind::Save, LOCAL, |e| {
        e.mark_bundle_saved(bundle)
    });
    assert!(appended);
    assert!(!store.bundle_is_dirty(bundle, LOCAL));
    assert!(store.bundle_is_dirty(bundle, SHARED));
}

// ── Ids of one store ─────────────────────────────────────────────────────────

#[test]
fn two_stores_have_different_generations() {
    assert_ne!(Store::new().generation(), Store::new().generation());
}

#[test]
fn ids_of_another_store_are_unknown_to_every_read_and_every_write() {
    let mut mine = Store::new();
    let own_file = load_ok(&mut mine, &Spec::new("p/m.properties", "m", None));
    let own_bundle = bundle_of(&mine, own_file);

    let mut other = Store::new();
    let foreign_file = load_ok(&mut other, &Spec::new("q/n.properties", "n", None));
    let foreign_bundle = bundle_of(&other, foreign_file);

    // reads
    assert_eq!(mine.file_path(foreign_file), None);
    assert_eq!(mine.file_bytes(foreign_file), None);
    assert_eq!(mine.file_encoding(foreign_file), None);
    assert_eq!(mine.file_fingerprint(foreign_file), None);
    assert_eq!(mine.file_assignment(foreign_file), None);
    assert!(mine.file_lines(foreign_file).is_empty());
    assert_eq!(mine.bundle_folder(foreign_bundle), None);
    assert_eq!(mine.bundle_name(foreign_bundle), None);
    assert_eq!(mine.bundle_excluded(foreign_bundle), None);
    assert!(mine.files_of(foreign_bundle).is_empty());
    assert!(mine.encoding_versions(foreign_file).is_empty());
    assert!(mine.assignment_versions(foreign_file).is_empty());
    assert!(mine.excluded_versions(foreign_bundle).is_empty());
    assert_eq!(mine.encoding_base(foreign_file, DISK), None);
    assert!(!mine.file_is_dirty(foreign_file, LOCAL));
    assert!(!mine.bundle_is_dirty(foreign_bundle, LOCAL));

    // writes
    assert!(!user(&mut mine, |e| e.set_encoding(foreign_file, Encoding::Utf8)));
    assert!(!user(&mut mine, |e| e.exclude_bundle(foreign_bundle, true)));
    assert!(!user(&mut mine, |e| e.mark_file_saved(foreign_file)));
    assert!(!user(&mut mine, |e| e.mark_bundle_saved(foreign_bundle)));

    // and the rows of this store at the same indices are untouched
    assert_eq!(mine.file_encoding(own_file), Some(Encoding::Latin1));
    assert_eq!(mine.bundle_excluded(own_bundle), Some(false));
    assert_eq!(mine.encoding_versions(own_file).len(), 1);
}

#[test]
fn ids_from_before_a_rebuild_are_unknown_in_the_new_store() {
    let mut old = Store::new();
    let old_file = load_ok(&mut old, &Spec::new("p/m.properties", "m", Some("de")));
    let old_text = old.file_assignment(old_file).unwrap().locale.unwrap();

    // a rebuild: a new store loads the same file again
    let mut new = Store::new();
    let new_file = load_ok(&mut new, &Spec::new("p/m.properties", "m", Some("de")));

    assert_eq!(new.file_path(old_file), None);
    assert_eq!(new.text(old_text), None);
    assert_eq!(new.file_path(new_file), Some(Path::new("p/m.properties")));
}

#[test]
fn a_text_path_or_bytes_id_of_another_store_cannot_be_resolved() {
    let mut mine = Store::new();
    load_ok(&mut mine, &Spec::new("p/m_de.properties", "m", Some("de")));

    let mut other = Store::new();
    let file = load_ok(&mut other, &Spec::new("q/n_fr.properties", "n", Some("fr")));
    let foreign_text = other.file_assignment(file).unwrap().locale.unwrap();

    assert_eq!(other.text(foreign_text), Some("fr"));
    assert_eq!(mine.text(foreign_text), None);
}

#[test]
fn the_assignment_as_a_value_is_comparable() {
    let mut store = Store::new();
    let first = load_ok(&mut store, &Spec::new("p/m_de.properties", "m", Some("de")));
    let second = load_ok(
        &mut store,
        &Spec::new("p/m_de2.properties", "m", Some("de")),
    );
    let a: BundleAssignment = store.file_assignment(first).unwrap();
    let b: BundleAssignment = store.file_assignment(second).unwrap();
    // the same bundle, the same locale text, the same confidence
    assert_eq!(a, b);
}

// ── Reload: a new snapshot of the disk ───────────────────────────────────────

/// What `app` does after it read a file again.
fn reload(
    store: &mut Store,
    file: FileId,
    bytes: &[u8],
    encoding: Encoding,
    lines: &[(&str, LineEnding)],
) -> (bool, EditId) {
    let lines: Vec<LineInput<'_>> = lines
        .iter()
        .map(|&(content, ending)| LineInput { content, ending })
        .collect();
    let input = FileReload {
        bytes,
        encoding,
        fingerprint: fingerprint(bytes),
        lines: &lines,
    };
    edit_as(store, EditKind::Reload, DISK, |e| {
        e.reload_file(file, input)
    })
}

fn contents(store: &Store, file: FileId) -> Vec<(String, LineEnding)> {
    store
        .file_lines(file)
        .iter()
        .map(|l| (l.content.to_owned(), l.ending))
        .collect()
}

fn snapshot_contents(store: &Store, file: FileId, snapshot: usize) -> Vec<String> {
    store
        .file_lines_in_snapshot(file, snapshot)
        .iter()
        .map(|l| l.content.to_owned())
        .collect()
}

#[test]
fn a_new_file_has_one_snapshot() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/x.properties", "x", None).lines(&[("a=1", Lf), ("b=2", Lf)]),
    );
    assert_eq!(store.snapshots(file), 1);
    assert_eq!(snapshot_contents(&store, file, 0), ["a=1", "b=2"]);
    assert!(store.file_lines_in_snapshot(file, 1).is_empty());
}

#[test]
fn a_reload_that_finds_the_same_disk_changes_nothing() {
    let mut store = Store::new();
    let spec = Spec::new("p/x.properties", "x", None)
        .bytes(b"a=1\nb=2\n")
        .lines(&[("a=1", Lf), ("b=2", Lf)]);
    let file = load_ok(&mut store, &spec);

    let (changed, _) = reload(
        &mut store,
        file,
        b"a=1\nb=2\n",
        Encoding::Latin1,
        &[("a=1", Lf), ("b=2", Lf)],
    );

    assert!(!changed);
    assert_eq!(store.snapshots(file), 1);
    assert_eq!(store.fingerprint_versions(file).len(), 1);
    assert_eq!(store.bytes_versions(file).len(), 1);
    assert_eq!(store.encoding_versions(file).len(), 1);
}

#[test]
fn a_reload_with_new_content_adds_a_snapshot_and_keeps_the_old_one() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/x.properties", "x", None)
            .bytes(b"a=1\nb=2\n")
            .lines(&[("a=1", Lf), ("b=2", Lf)]),
    );

    let (changed, _) = reload(
        &mut store,
        file,
        b"a=1\nb=2\nc=3\n",
        Encoding::Latin1,
        &[("a=1", Lf), ("b=2", Lf), ("c=3", Lf)],
    );

    assert!(changed);
    assert_eq!(store.snapshots(file), 2);
    assert_eq!(
        contents(&store, file),
        [
            ("a=1".to_owned(), Lf),
            ("b=2".to_owned(), Lf),
            ("c=3".to_owned(), Lf)
        ]
    );
    assert_eq!(snapshot_contents(&store, file, 0), ["a=1", "b=2"]);
    assert_eq!(snapshot_contents(&store, file, 1), ["a=1", "b=2", "c=3"]);
}

#[test]
fn a_changed_order_in_the_file_is_just_another_snapshot() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/x.properties", "x", None).lines(&[("a=1", Lf), ("b=2", Lf), ("c=3", Lf)]),
    );

    reload(
        &mut store,
        file,
        b"c=3\na=1\nb=2\n",
        Encoding::Latin1,
        &[("c=3", Lf), ("a=1", Lf), ("b=2", Lf)],
    );

    assert_eq!(snapshot_contents(&store, file, 1), ["c=3", "a=1", "b=2"]);
    assert_eq!(snapshot_contents(&store, file, 0), ["a=1", "b=2", "c=3"]);
    assert_eq!(store.file_lines(file)[0].content, "c=3");
}

#[test]
fn a_change_of_only_a_line_ending_is_a_change() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/x.properties", "x", None).lines(&[("a=1", Lf)]),
    );
    let (changed, _) = reload(
        &mut store,
        file,
        b"a=1\r\n",
        Encoding::Latin1,
        &[("a=1", CrLf)],
    );
    assert!(changed);
    assert_eq!(store.snapshots(file), 2);
    assert_eq!(store.file_lines(file)[0].ending, CrLf);
}

#[test]
fn a_reload_replaces_bytes_and_fingerprint_and_keeps_the_old_ones() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/x.properties", "x", None).bytes(b"a=1\n"),
    );

    reload(&mut store, file, b"a=2\n", Encoding::Latin1, &[("a=2", Lf)]);

    assert_eq!(store.file_bytes(file), Some(&b"a=2\n"[..]));
    assert_eq!(store.file_fingerprint(file), Some(fingerprint(b"a=2\n")));
    let bytes = store.bytes_versions(file);
    assert_eq!(bytes.len(), 2);
    assert_eq!(store.bytes(bytes[0].value), Some(&b"a=1\n"[..]));
    assert_eq!((bytes[1].source, bytes[1].kind), (DISK, EditKind::Reload));
    let fingerprints = store.fingerprint_versions(file);
    assert_eq!(fingerprints.len(), 2);
    assert_eq!(fingerprints[0].value, fingerprint(b"a=1\n"));
}

#[test]
fn everything_a_reload_brings_belongs_to_one_edit() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/x.properties", "x", None).encoding(Encoding::Latin1),
    );

    let (_, edit) = reload(&mut store, file, b"a=2\n", Encoding::Utf8, &[("a=2", Lf)]);

    assert_eq!(store.bytes_versions(file)[1].edit, edit);
    assert_eq!(store.fingerprint_versions(file)[1].edit, edit);
    assert_eq!(store.encoding_versions(file)[1].edit, edit);
}

#[test]
fn a_reload_keeps_the_encoding_the_user_chose() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/x.properties", "x", None).encoding(Encoding::Latin1),
    );
    user(&mut store, |e| e.set_encoding(file, Encoding::Utf8));

    // the disk is now detected as Windows-1252
    reload(
        &mut store,
        file,
        b"a=2\n",
        Encoding::Windows1252,
        &[("a=2", Lf)],
    );

    assert_eq!(store.file_encoding(file), Some(Encoding::Utf8));
    assert_eq!(store.encoding_base(file, DISK), Some(Encoding::Windows1252));
    let sources: Vec<(Encoding, Source)> = store
        .encoding_versions(file)
        .iter()
        .map(|v| (v.value, v.source))
        .collect();
    assert_eq!(
        sources,
        [
            (Encoding::Latin1, DISK),
            (Encoding::Utf8, USER),
            (Encoding::Windows1252, DISK),
            (Encoding::Utf8, USER),
        ]
    );
}

#[test]
fn a_reload_that_detects_the_same_encoding_leaves_the_choice_of_the_user() {
    let mut store = Store::new();
    let file = load_ok(
        &mut store,
        &Spec::new("p/x.properties", "x", None).encoding(Encoding::Latin1),
    );
    user(&mut store, |e| e.set_encoding(file, Encoding::Utf8));

    reload(&mut store, file, b"a=2\n", Encoding::Latin1, &[("a=2", Lf)]);

    assert_eq!(store.file_encoding(file), Some(Encoding::Utf8));
    assert_eq!(store.encoding_versions(file).len(), 2);
}

#[test]
fn a_reload_leaves_the_assignment_alone() {
    let mut store = Store::new();
    let file = load_ok(&mut store, &Spec::new("p/m_de.properties", "m", Some("de")));
    let before = store.file_assignment(file);

    reload(&mut store, file, b"a=2\n", Encoding::Latin1, &[("a=2", Lf)]);

    assert_eq!(store.file_assignment(file), before);
    assert_eq!(store.assignment_versions(file).len(), 1);
}

#[test]
fn a_reload_does_not_touch_other_files() {
    let mut store = Store::new();
    let first = load_ok(
        &mut store,
        &Spec::new("p/a.properties", "a", None).lines(&[("x=1", Lf)]),
    );
    let second = load_ok(
        &mut store,
        &Spec::new("p/b.properties", "b", None).lines(&[("y=2", Lf)]),
    );

    reload(
        &mut store,
        first,
        b"x=9\n",
        Encoding::Latin1,
        &[("x=9", Lf)],
    );

    assert_eq!(snapshot_contents(&store, second, 0), ["y=2"]);
    assert_eq!(store.snapshots(second), 1);
    assert_eq!(store.fingerprint_versions(second).len(), 1);
}

#[test]
fn a_reload_of_an_unknown_file_changes_nothing() {
    let mut store = Store::new();
    let own = load_ok(&mut store, &Spec::new("p/m.properties", "m", None));

    let mut other = Store::new();
    let foreign = load_ok(&mut other, &Spec::new("q/n.properties", "n", None));

    let (changed, _) = reload(
        &mut store,
        foreign,
        b"z=1\n",
        Encoding::Utf8,
        &[("z=1", Lf)],
    );

    assert!(!changed);
    assert_eq!(store.snapshots(own), 1);
    assert_eq!(store.file_encoding(own), Some(Encoding::Latin1));
}

#[test]
fn the_snapshots_of_an_unknown_file_are_empty() {
    let store = Store::new();
    let mut other = Store::new();
    let foreign = load_ok(&mut other, &Spec::new("q/n.properties", "n", None));
    assert_eq!(store.snapshots(foreign), 0);
    assert!(store.file_lines_in_snapshot(foreign, 0).is_empty());
    assert!(store.bytes_versions(foreign).is_empty());
    assert!(store.fingerprint_versions(foreign).is_empty());
}
