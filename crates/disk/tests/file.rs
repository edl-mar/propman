//! Specification of whole-file reading and writing (see `disk::file`), including the golden
//! roundtrip over all fixtures.

mod common;

use std::path::{Path, PathBuf};

use common::TempDir;
use disk::encoding::{DecodeError, Encoding};
use disk::file::{ReadError, read_file, write_file};
use disk::lines::{Line, split_lines};

fn fixtures(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel)
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn replace_bytes(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    let at = haystack
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("needle not found in the fixture");
    let mut out = haystack[..at].to_vec();
    out.extend_from_slice(with);
    out.extend_from_slice(&haystack[at + needle.len()..]);
    out
}

// ── read_file ────────────────────────────────────────────────────────────────

#[test]
fn read_file_detects_the_encoding_and_decodes() {
    let it = read_file(&fixtures("edge/encodings/messages_it.properties"), None).unwrap();
    assert_eq!(it.encoding, Encoding::Windows1252);
    assert!(it.text.contains("Prezzo: 5 \u{20ac}"));
}

#[test]
fn read_file_strips_the_bom_from_the_text() {
    let de = read_file(&fixtures("edge/encodings/messages_de.properties"), None).unwrap();
    assert_eq!(de.encoding, Encoding::Utf8Bom);
    assert!(de.text.starts_with("# UTF-8 with BOM"));
}

#[test]
fn read_file_with_a_hint_overrides_detection() {
    let path = fixtures("edge/encodings/messages_it.properties");
    let as_latin1 = read_file(&path, Some(Encoding::Latin1)).unwrap();
    assert_eq!(as_latin1.encoding, Encoding::Latin1);
    assert!(!as_latin1.text.contains('\u{20ac}'));
}

#[test]
fn read_file_reports_a_missing_file_as_an_io_error() {
    let dir = TempDir::new();
    let result = read_file(&dir.path().join("missing.properties"), None);
    match result {
        Err(ReadError::Io(e)) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
        other => panic!("expected an io error, got {other:?}"),
    }
}

#[test]
fn read_file_reports_a_forced_but_impossible_encoding_as_a_decode_error() {
    let path = fixtures("edge/encodings/messages_fr.properties"); // Latin-1, invalid UTF-8
    let result = read_file(&path, Some(Encoding::Utf8));
    assert!(matches!(
        result,
        Err(ReadError::Decode(DecodeError::InvalidUtf8))
    ));
}

// ── The golden roundtrip ─────────────────────────────────────────────────────

#[test]
fn golden_roundtrip_every_fixture_is_saved_byte_for_byte() {
    let mut files = Vec::new();
    files_under(&fixtures(""), &mut files);
    assert!(files.len() >= 10, "fixtures not found, got {files:?}");

    let out = TempDir::new();
    for (i, path) in files.iter().enumerate() {
        let original = std::fs::read(path).unwrap();
        let read = read_file(path, None).unwrap();
        let target = out.path().join(format!("out{i}.properties"));
        write_file(&target, read.encoding, &split_lines(&read.text)).unwrap();
        assert_eq!(
            std::fs::read(&target).unwrap(),
            original,
            "{}",
            path.display()
        );
    }
}

#[test]
fn the_roundtrip_also_holds_with_a_forced_single_byte_encoding() {
    // Latin-1 accepts every byte; Windows-1252 every file without its five undefined bytes.
    let path = fixtures("edge/encodings/messages_it.properties");
    let original = std::fs::read(&path).unwrap();
    let out = TempDir::new();
    for hint in [Encoding::Latin1, Encoding::Windows1252] {
        let read = read_file(&path, Some(hint)).unwrap();
        let target = out.path().join(format!("{hint:?}.properties"));
        write_file(&target, read.encoding, &split_lines(&read.text)).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), original, "{hint:?}");
    }
}

// ── Changing one line changes only that line ─────────────────────────────────

/// Reads `rel`, replaces the content of the line starting with `prefix`, writes the file
/// into a temp dir and returns (original bytes, saved bytes).
fn edit_line(rel: &str, prefix: &str, new_content: &str) -> (Vec<u8>, Vec<u8>) {
    let path = fixtures(rel);
    let original = std::fs::read(&path).unwrap();
    let read = read_file(&path, None).unwrap();
    let mut lines: Vec<Line<'_>> = split_lines(&read.text);
    let index = lines
        .iter()
        .position(|l| l.content.starts_with(prefix))
        .unwrap_or_else(|| panic!("no line starting with {prefix:?}"));
    lines[index].content = new_content;

    let out = TempDir::new();
    let target = out.path().join("out.properties");
    write_file(&target, read.encoding, &lines).unwrap();
    (original, std::fs::read(&target).unwrap())
}

#[test]
fn editing_a_line_keeps_the_bytes_of_all_other_lines() {
    // Latin-1 with CRLF: the edited line keeps its own CRLF, all others stay as they were.
    let (original, saved) = edit_line(
        "edge/encodings/messages_fr.properties",
        "greeting=",
        "greeting=Salut",
    );
    assert_eq!(
        saved,
        replace_bytes(&original, b"greeting=Bonjour", b"greeting=Salut")
    );
}

#[test]
fn editing_in_a_file_with_mixed_line_endings_keeps_every_ending() {
    let (original, saved) = edit_line(
        "edge/encodings/messages_pl.properties",
        "farewell=",
        "farewell=Pa",
    );
    assert_eq!(
        saved,
        replace_bytes(&original, b"farewell=Do widzenia", b"farewell=Pa")
    );
}

#[test]
fn a_character_the_encoding_cannot_hold_is_written_as_an_escape() {
    let (original, saved) = edit_line(
        "edge/encodings/messages_fr.properties",
        "farewell=",
        "farewell=5 \u{20ac}",
    );
    assert_eq!(
        saved,
        replace_bytes(
            &original,
            b"farewell=\xc0 bient\xf4t",
            b"farewell=5 \\u20AC"
        )
    );
}

#[test]
fn the_bom_survives_an_edit() {
    let (original, saved) = edit_line(
        "edge/encodings/messages_de.properties",
        "greeting=",
        "greeting=Servus",
    );
    assert!(saved.starts_with(b"\xef\xbb\xbf"));
    assert_eq!(
        saved,
        replace_bytes(&original, b"greeting=Hallo", b"greeting=Servus")
    );
}

// ── write_file ───────────────────────────────────────────────────────────────

#[test]
fn write_file_replaces_atomically_and_leaves_no_other_file() {
    let dir = TempDir::new();
    dir.write("a.properties", b"old content");
    let lines = split_lines("a=1\r\nb=2\r\n");
    write_file(&dir.path().join("a.properties"), Encoding::Utf8, &lines).unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("a.properties")).unwrap(),
        b"a=1\r\nb=2\r\n"
    );
    assert_eq!(dir.entries(), ["a.properties"]);
}

#[test]
fn write_file_into_a_missing_directory_is_an_error() {
    let dir = TempDir::new();
    let lines = split_lines("a=1\n");
    assert!(
        write_file(
            &dir.path().join("missing/a.properties"),
            Encoding::Utf8,
            &lines
        )
        .is_err()
    );
    assert!(dir.entries().is_empty());
}
