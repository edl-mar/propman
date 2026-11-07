//! Specification of file discovery (see `disk::discover`).

mod common;

use std::path::{Path, PathBuf};

use common::TempDir;
use disk::discover::{DiscoverOptions, Discovery, discover};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn recursive() -> DiscoverOptions {
    DiscoverOptions { recursive: true }
}

fn flat() -> DiscoverOptions {
    DiscoverOptions { recursive: false }
}

/// File paths relative to `root`, with `/` as separator.
fn relative(root: &Path, found: &Discovery) -> Vec<String> {
    found
        .files
        .iter()
        .map(|p| {
            p.strip_prefix(root)
                .unwrap_or_else(|_| panic!("{} does not start with the root", p.display()))
                .display()
                .to_string()
                .replace('\\', "/")
        })
        .collect()
}

// ── What counts as a file ────────────────────────────────────────────────────

#[test]
fn finds_properties_files_recursively_sorted_by_path() {
    let dir = TempDir::new();
    // created in reverse order on purpose: the result must not depend on creation order
    for rel in [
        "z.properties",
        "sub/b.properties",
        "sub/a.properties",
        "a.properties",
    ] {
        dir.touch(rel);
    }
    let found = discover(dir.path(), &recursive());
    assert_eq!(
        relative(dir.path(), &found),
        [
            "a.properties",
            "sub/a.properties",
            "sub/b.properties",
            "z.properties"
        ]
    );
    assert!(found.errors.is_empty());
}

#[test]
fn other_files_and_directories_are_ignored() {
    let dir = TempDir::new();
    for rel in [
        "a.txt",
        "a.properties.bak",
        "Makefile",
        ".propman.toml",
        "ok.properties",
    ] {
        dir.touch(rel);
    }
    dir.mkdir("looks_like_a_file.properties");
    let found = discover(dir.path(), &recursive());
    assert_eq!(relative(dir.path(), &found), ["ok.properties"]);
}

#[test]
fn the_extension_must_be_exactly_lowercase_like_in_java() {
    let dir = TempDir::new();
    dir.touch("upper.PROPERTIES");
    dir.touch("lower.properties");
    let found = discover(dir.path(), &recursive());
    assert_eq!(relative(dir.path(), &found), ["lower.properties"]);
}

#[test]
fn empty_files_are_found() {
    let dir = TempDir::new();
    dir.touch("empty.properties");
    assert_eq!(
        relative(dir.path(), &discover(dir.path(), &recursive())),
        ["empty.properties"]
    );
}

#[test]
fn an_empty_directory_has_no_files_and_no_errors() {
    let dir = TempDir::new();
    let found = discover(dir.path(), &recursive());
    assert!(found.files.is_empty());
    assert!(found.errors.is_empty());
}

// ── Recursion ────────────────────────────────────────────────────────────────

#[test]
fn non_recursive_lists_only_the_direct_children() {
    let dir = TempDir::new();
    dir.touch("top.properties");
    dir.touch("sub/nested.properties");
    let found = discover(dir.path(), &flat());
    assert_eq!(relative(dir.path(), &found), ["top.properties"]);
}

#[test]
fn recursive_is_the_default() {
    assert_eq!(DiscoverOptions::default(), recursive());
}

// ── Hidden files and directories ─────────────────────────────────────────────

#[test]
fn hidden_files_are_skipped() {
    let dir = TempDir::new();
    dir.touch(".hidden.properties");
    dir.touch("sub/.also_hidden.properties");
    dir.touch("visible.properties");
    let found = discover(dir.path(), &recursive());
    assert_eq!(relative(dir.path(), &found), ["visible.properties"]);
}

#[test]
fn hidden_directories_are_skipped_with_everything_below() {
    let dir = TempDir::new();
    dir.touch(".git/a.properties");
    dir.touch("sub/.cache/deeper/b.properties");
    dir.touch("sub/c.properties");
    let found = discover(dir.path(), &recursive());
    assert_eq!(relative(dir.path(), &found), ["sub/c.properties"]);
}

#[test]
fn the_root_itself_is_never_treated_as_hidden() {
    let dir = TempDir::new();
    let root = dir.path().join(".hidden_root");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("a.properties"), b"").unwrap();
    let found = discover(&root, &recursive());
    assert_eq!(relative(&root, &found), ["a.properties"]);
}

#[cfg(windows)]
#[test]
fn files_and_directories_with_the_windows_hidden_attribute_are_skipped() {
    let dir = TempDir::new();
    dir.touch("plain.properties");
    dir.touch("attr_file.properties");
    dir.touch("attr_dir/inside.properties");
    for rel in ["attr_file.properties", "attr_dir"] {
        let status = std::process::Command::new("attrib")
            .arg("+h")
            .arg(dir.path().join(rel))
            .status()
            .unwrap();
        assert!(status.success());
    }
    let found = discover(dir.path(), &recursive());
    assert_eq!(relative(dir.path(), &found), ["plain.properties"]);
}

// ── Root handling and errors ─────────────────────────────────────────────────

#[test]
fn result_paths_start_with_the_root_as_given() {
    let dir = TempDir::new();
    dir.touch("a.properties");
    let found = discover(dir.path(), &recursive());
    assert!(found.files.iter().all(|p| p.starts_with(dir.path())));
}

#[test]
fn a_relative_root_with_parent_components_works() {
    // `..` starts with a dot, but the root is never judged
    let root = Path::new("../../fixtures/project");
    let found = discover(root, &recursive());
    assert_eq!(found.files.len(), 12, "{found:?}");
}

#[test]
fn a_missing_root_is_one_error_and_no_files() {
    let dir = TempDir::new();
    let missing = dir.path().join("does_not_exist");
    let found = discover(&missing, &recursive());
    assert!(found.files.is_empty());
    assert_eq!(found.errors.len(), 1, "{found:?}");
    assert_eq!(found.errors[0].path.as_deref(), Some(missing.as_path()));
    assert!(!found.errors[0].message.is_empty());
}

#[test]
fn a_root_that_is_a_file_is_an_error() {
    let dir = TempDir::new();
    dir.touch("a.properties");
    let root = dir.path().join("a.properties");
    let found = discover(&root, &recursive());
    assert!(found.files.is_empty());
    assert_eq!(found.errors.len(), 1, "{found:?}");
}

// ── Real fixtures ────────────────────────────────────────────────────────────

fn fixtures(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(sub)
}

#[test]
fn the_project_fixtures_are_found() {
    let root = fixtures("project");
    let found = discover(&root, &recursive());
    assert!(found.errors.is_empty(), "{found:?}");
    assert_eq!(
        relative(&root, &found),
        [
            "messages.properties",
            "messages_de.properties",
            "messages_fr.properties",
            "messages_it.properties",
            "stripping.properties",
            "sub_folder/translations.properties",
            "sub_folder/translations_de.properties",
            "sub_folder/translations_si.properties",
            "testbundle.properties",
            "testbundle_de.properties",
            "testbundle_fr.properties",
            "testbundle_it.properties",
        ]
    );
}

#[test]
fn the_project_fixtures_non_recursive_skip_the_sub_folder() {
    let root = fixtures("project");
    let found = discover(&root, &flat());
    assert_eq!(found.files.len(), 9, "{found:?}");
    assert!(relative(&root, &found).iter().all(|p| !p.contains('/')));
}

#[test]
fn the_edge_fixtures_are_found() {
    let root = fixtures("edge");
    let found = discover(&root, &recursive());
    assert_eq!(found.files.len(), 5, "{found:?}");
}
