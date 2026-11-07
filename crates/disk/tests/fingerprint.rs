//! Specification of change detection (see `disk::fingerprint`) and of how `read_file` and
//! `write_file` use it.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use common::TempDir;
use disk::encoding::Encoding;
use disk::file::{WriteError, read_file, write_file};
use disk::fingerprint::{Check, Fingerprint, check, hash_bytes};
use disk::lines::split_lines;

// ── Helpers ──────────────────────────────────────────────────────────────────

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel)
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path).unwrap().modified().unwrap()
}

fn set_modified(path: &Path, time: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

/// Writes `content`, reads it back and returns the file with its fingerprint.
fn file_with(dir: &TempDir, content: &[u8]) -> (PathBuf, Fingerprint) {
    dir.write("a.properties", content);
    let path = dir.path().join("a.properties");
    let fingerprint = read_file(&path, None).unwrap().fingerprint;
    (path, fingerprint)
}

fn save(
    path: &Path,
    text: &str,
    expected: Option<&Fingerprint>,
) -> Result<Fingerprint, WriteError> {
    write_file(path, Encoding::Utf8, &split_lines(text), expected)
}

// ── The hash ─────────────────────────────────────────────────────────────────

#[test]
fn the_hash_is_fnv_1a_64_and_matches_the_published_test_vectors() {
    assert_eq!(hash_bytes(b""), 0xcbf29ce484222325);
    assert_eq!(hash_bytes(b"a"), 0xaf63dc4c8601ec8c);
    assert_eq!(hash_bytes(b"foobar"), 0x85944171f73967e8);
}

// ── What read_file records ───────────────────────────────────────────────────

#[test]
fn the_fingerprint_describes_the_raw_bytes_not_the_decoded_text() {
    // UTF-8 with BOM: the BOM is not in the text, but it is in the bytes.
    let path = fixture("edge/encodings/messages_de.properties");
    let bytes = std::fs::read(&path).unwrap();
    let fingerprint = read_file(&path, None).unwrap().fingerprint;
    assert_eq!(fingerprint.len, bytes.len() as u64);
    assert_eq!(fingerprint.hash, hash_bytes(&bytes));
}

#[test]
fn the_stamp_is_the_modification_time_of_the_file() {
    let path = fixture("edge/encodings/messages_fr.properties");
    let fingerprint = read_file(&path, None).unwrap().fingerprint;
    assert_eq!(fingerprint.modified, Some(modified(&path)));
}

// ── check ────────────────────────────────────────────────────────────────────

#[test]
fn an_untouched_file_is_unchanged() {
    let dir = TempDir::new();
    let (path, fingerprint) = file_with(&dir, b"a=1\n");
    assert_eq!(
        check(&path, &fingerprint).unwrap(),
        Check::Unchanged(fingerprint)
    );
}

#[test]
fn a_new_modification_time_with_the_same_content_is_unchanged_and_refreshes_the_stamp() {
    // what `git checkout` does
    let dir = TempDir::new();
    let (path, fingerprint) = file_with(&dir, b"a=1\n");
    let later = modified(&path) + Duration::from_secs(3600);
    set_modified(&path, later);

    match check(&path, &fingerprint).unwrap() {
        Check::Unchanged(refreshed) => {
            assert_eq!(refreshed.modified, Some(later));
            assert_eq!(
                (refreshed.len, refreshed.hash),
                (fingerprint.len, fingerprint.hash)
            );
        }
        other => panic!("expected Unchanged, got {other:?}"),
    }
}

#[test]
fn a_change_with_the_same_length_and_the_same_modification_time_is_still_detected() {
    // within the time resolution of the file system the stamp cannot tell; the hash can
    let dir = TempDir::new();
    let (path, fingerprint) = file_with(&dir, b"a=1\n");
    let original_time = modified(&path);
    std::fs::write(&path, b"a=2\n").unwrap();
    set_modified(&path, original_time);

    assert_eq!(check(&path, &fingerprint).unwrap(), Check::Changed);
}

#[test]
fn a_change_of_the_length_is_detected() {
    let dir = TempDir::new();
    let (path, fingerprint) = file_with(&dir, b"a=1\n");
    std::fs::write(&path, b"a=1\nb=2\n").unwrap();
    assert_eq!(check(&path, &fingerprint).unwrap(), Check::Changed);
}

#[test]
fn restoring_the_old_content_makes_the_file_unchanged_again() {
    let dir = TempDir::new();
    let (path, fingerprint) = file_with(&dir, b"a=1\n");
    std::fs::write(&path, b"changed\n").unwrap();
    assert_eq!(check(&path, &fingerprint).unwrap(), Check::Changed);
    std::fs::write(&path, b"a=1\n").unwrap();
    assert!(matches!(
        check(&path, &fingerprint).unwrap(),
        Check::Unchanged(_)
    ));
}

#[test]
fn a_deleted_file_is_reported_as_deleted() {
    let dir = TempDir::new();
    let (path, fingerprint) = file_with(&dir, b"a=1\n");
    std::fs::remove_file(&path).unwrap();
    assert_eq!(check(&path, &fingerprint).unwrap(), Check::Deleted);
}

#[test]
fn other_errors_are_returned_as_they_are() {
    let dir = TempDir::new();
    let (_, fingerprint) = file_with(&dir, b"a=1\n");
    dir.mkdir("a_directory");
    assert!(check(&dir.path().join("a_directory"), &fingerprint).is_err());
}

// ── write_file with an expected fingerprint ──────────────────────────────────

#[test]
fn a_matching_fingerprint_lets_the_write_through_and_returns_the_new_fingerprint() {
    let dir = TempDir::new();
    let (path, fingerprint) = file_with(&dir, b"a=1\n");

    let new = save(&path, "a=2\r\nb=3\r\n", Some(&fingerprint)).unwrap();

    let written = std::fs::read(&path).unwrap();
    assert_eq!(written, b"a=2\r\nb=3\r\n");
    assert_eq!(new.len, written.len() as u64);
    assert_eq!(new.hash, hash_bytes(&written));
    assert_eq!(new.modified, Some(modified(&path)));
    assert_eq!(check(&path, &new).unwrap(), Check::Unchanged(new));
}

#[test]
fn the_new_fingerprint_covers_the_encoded_bytes_including_escapes() {
    let dir = TempDir::new();
    let (path, fingerprint) = file_with(&dir, b"a=1\n");
    // the euro sign does not exist in Latin-1 and is written as an escape
    let new = write_file(
        &path,
        Encoding::Latin1,
        &split_lines("price=5 \u{20ac}\n"),
        Some(&fingerprint),
    )
    .unwrap();
    let written = std::fs::read(&path).unwrap();
    assert_eq!(written, b"price=5 \\u20AC\n");
    assert_eq!(new.hash, hash_bytes(&written));
}

#[test]
fn consecutive_saves_chain_and_a_stale_fingerprint_is_refused() {
    let dir = TempDir::new();
    let (path, first) = file_with(&dir, b"v=0\n");
    let second = save(&path, "v=1\n", Some(&first)).unwrap();
    let third = save(&path, "v=2\n", Some(&second)).unwrap();

    assert!(matches!(
        save(&path, "v=stale\n", Some(&first)),
        Err(WriteError::Changed)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), b"v=2\n");
    assert!(matches!(check(&path, &third).unwrap(), Check::Unchanged(_)));
}

#[test]
fn an_outside_change_refuses_the_write_and_leaves_the_file_as_it_is() {
    let dir = TempDir::new();
    let (path, fingerprint) = file_with(&dir, b"a=1\n");
    std::fs::write(&path, b"changed by someone else\n").unwrap();

    let result = save(&path, "a=mine\n", Some(&fingerprint));

    assert!(matches!(result, Err(WriteError::Changed)), "{result:?}");
    assert_eq!(std::fs::read(&path).unwrap(), b"changed by someone else\n");
    assert_eq!(dir.entries(), ["a.properties"]);
}

#[test]
fn an_outside_delete_refuses_the_write_and_does_not_recreate_the_file() {
    let dir = TempDir::new();
    let (path, fingerprint) = file_with(&dir, b"a=1\n");
    std::fs::remove_file(&path).unwrap();

    let result = save(&path, "a=mine\n", Some(&fingerprint));

    assert!(matches!(result, Err(WriteError::Deleted)), "{result:?}");
    assert!(dir.entries().is_empty());
}

#[test]
fn a_touch_without_content_change_is_no_conflict() {
    let dir = TempDir::new();
    let (path, fingerprint) = file_with(&dir, b"a=1\n");
    set_modified(&path, modified(&path) + Duration::from_secs(3600));
    assert!(save(&path, "a=2\n", Some(&fingerprint)).is_ok());
    assert_eq!(std::fs::read(&path).unwrap(), b"a=2\n");
}

// ── write_file without an expected fingerprint ───────────────────────────────

#[test]
fn without_an_expectation_the_file_is_overwritten_or_created_unchecked() {
    let dir = TempDir::new();
    let path = dir.path().join("new.properties");
    let created = save(&path, "a=1\n", None).unwrap();
    assert_eq!(created.hash, hash_bytes(b"a=1\n"));

    std::fs::write(&path, b"changed by someone else\n").unwrap();
    save(&path, "a=2\n", None).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"a=2\n");
}

#[test]
fn io_errors_stay_io_errors() {
    let dir = TempDir::new();
    let path = dir.path().join("missing_dir").join("a.properties");
    assert!(matches!(save(&path, "a=1\n", None), Err(WriteError::Io(_))));
    assert!(dir.entries().is_empty());
}
