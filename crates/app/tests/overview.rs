//! Specification of the bundle overview as a list (see `app::overview`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use app::{Confidence, Encoding, OverviewItem, Session, overview};
use common::{TempDir, fixtures};

fn open(folder: &Path) -> Session {
    let (session, problems) = Session::open(folder);
    assert!(problems.is_empty(), "{problems:?}");
    session
}

/// One short text per item, to compare the order and the main content at a glance.
fn summary(items: &[OverviewItem]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            OverviewItem::Folder { path, bundles } => format!("folder {path} ({bundles})"),
            OverviewItem::Bundle { name, files, .. } => format!("bundle {name} ({files})"),
            OverviewItem::File { name, locale, .. } => format!("file {name} {locale:?}"),
        })
        .collect()
}

fn write(dir: &TempDir, relative: &str, bytes: &[u8]) {
    let path = dir.path().join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn folders_bundles_and_files_are_listed_in_order() {
    let session = open(&fixtures().join("project"));
    let expected = [
        "folder . (3)",
        "bundle messages (4)",
        "file messages.properties None",
        "file messages_de.properties Some(\"de\")",
        "file messages_fr.properties Some(\"fr\")",
        "file messages_it.properties Some(\"it\")",
        "bundle stripping (1)",
        "file stripping.properties None",
        "bundle testbundle (4)",
        "file testbundle.properties None",
        "file testbundle_de.properties Some(\"de\")",
        "file testbundle_fr.properties Some(\"fr\")",
        "file testbundle_it.properties Some(\"it\")",
        "folder sub_folder (1)",
        "bundle translations (3)",
        "file translations.properties None",
        "file translations_de.properties Some(\"de\")",
        "file translations_si.properties Some(\"si\")",
    ];
    assert_eq!(summary(&overview(&session)), expected);
}

#[test]
fn folders_are_relative_to_the_opened_folder_with_slashes() {
    let session = open(&fixtures());
    let items = overview(&session);

    let folders: Vec<&str> = items
        .iter()
        .filter_map(|item| match item {
            OverviewItem::Folder { path, .. } => Some(path.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(folders, ["edge/encodings", "project", "project/sub_folder"]);
    // 3 folders, 5 bundles and 17 files
    assert_eq!(items.len(), 25);
}

#[test]
fn the_opened_folder_itself_is_a_dot() {
    let dir = TempDir::new();
    write(&dir, "a.properties", b"a=1\n");
    let items = overview(&open(dir.path()));
    assert_eq!(
        items[0],
        OverviewItem::Folder {
            path: ".".to_owned(),
            bundles: 1
        }
    );
}

#[test]
fn a_file_carries_its_locale_confidence_encoding_and_number_of_lines() {
    let dir = TempDir::new();
    write(&dir, "single_file_de.properties", b"a=1\nb=2\n");
    let session = open(dir.path());

    let items = overview(&session);
    assert_eq!(items.len(), 3);
    match &items[2] {
        OverviewItem::File {
            name,
            locale,
            confidence,
            encoding,
            lines,
            ..
        } => {
            assert_eq!(name, "single_file_de.properties");
            assert_eq!(locale.as_deref(), Some("de"));
            // a locale next to no other file of its bundle is only the likely reading
            assert_eq!(*confidence, Confidence::Uncertain);
            assert_eq!(*encoding, Encoding::Utf8);
            assert_eq!(*lines, 2);
        }
        other => panic!("expected a file, got {other:?}"),
    }
}

#[test]
fn files_of_a_certain_bundle_are_certain() {
    let session = open(&fixtures().join("project"));
    for item in overview(&session) {
        if let OverviewItem::File { confidence, .. } = item {
            assert_eq!(confidence, Confidence::Certain);
        }
    }
}

#[test]
fn the_encoding_of_a_file_is_its_current_one() {
    let mut session = open(&fixtures().join("edge"));
    let file = session
        .store()
        .files()
        .into_iter()
        .find(|&f| {
            session
                .store()
                .file_path(f)
                .is_some_and(|p| p.ends_with("messages_fr.properties"))
        })
        .unwrap();
    assert!(session.set_encoding(file, Encoding::Utf8));

    let encoding = overview(&session)
        .into_iter()
        .find_map(|item| match item {
            OverviewItem::File { id, encoding, .. } if id == file => Some(encoding),
            _ => None,
        })
        .unwrap();
    assert_eq!(encoding, Encoding::Utf8);
}

#[test]
fn an_excluded_bundle_is_marked_and_keeps_its_files() {
    let mut session = open(&fixtures().join("project"));
    let bundle = session.store().bundles()[0];
    assert!(session.exclude_bundle(bundle, true));

    let items = overview(&session);
    let excluded: Vec<&str> = items
        .iter()
        .filter_map(|item| match item {
            OverviewItem::Bundle {
                name,
                excluded: true,
                ..
            } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(excluded, ["messages"]);
    // all 18 items are still there
    assert_eq!(items.len(), 18);
}

#[test]
fn bundles_that_are_not_excluded_say_so() {
    let session = open(&fixtures().join("project"));
    let any_excluded = overview(&session)
        .iter()
        .any(|item| matches!(item, OverviewItem::Bundle { excluded: true, .. }));
    assert!(!any_excluded);
}

#[test]
fn an_empty_folder_gives_an_empty_overview() {
    let dir = TempDir::new();
    assert!(overview(&open(dir.path())).is_empty());
}

#[test]
fn the_same_name_in_two_folders_gives_two_bundles_under_their_own_folder() {
    let dir = TempDir::new();
    write(&dir, "src/messages.properties", b"a=1\n");
    write(&dir, "target/messages.properties", b"a=1\n");

    let expected = [
        "folder src (1)",
        "bundle messages (1)",
        "file messages.properties None",
        "folder target (1)",
        "bundle messages (1)",
        "file messages.properties None",
    ];
    assert_eq!(summary(&overview(&open(dir.path()))), expected);
}
