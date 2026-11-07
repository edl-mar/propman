//! Specification of the physical line model (see `disk::lines`).

use LineEnding::{Cr, CrLf, Lf, None as NoEnd};
use disk::lines::{Line, LineEnding, join_lines, split_lines};

fn l(content: &str, ending: LineEnding) -> Line<'_> {
    Line { content, ending }
}

// ── Basic shapes ─────────────────────────────────────────────────────────────

#[test]
fn empty_text_has_no_lines() {
    assert_eq!(split_lines(""), vec![]);
}

#[test]
fn a_lone_terminator_is_one_empty_line() {
    assert_eq!(split_lines("\n"), vec![l("", Lf)]);
    assert_eq!(split_lines("\r\n"), vec![l("", CrLf)]);
    assert_eq!(split_lines("\r"), vec![l("", Cr)]);
}

#[test]
fn text_without_terminator_is_one_line_without_ending() {
    assert_eq!(split_lines("a"), vec![l("a", NoEnd)]);
}

#[test]
fn trailing_terminator_does_not_create_a_phantom_line() {
    assert_eq!(split_lines("a\n"), vec![l("a", Lf)]);
    assert_eq!(split_lines("a\r\n"), vec![l("a", CrLf)]);
    assert_eq!(split_lines("a\r"), vec![l("a", Cr)]);
}

#[test]
fn missing_final_terminator_only_affects_the_last_line() {
    assert_eq!(split_lines("a\nb"), vec![l("a", Lf), l("b", NoEnd)]);
}

#[test]
fn blank_lines_are_preserved() {
    assert_eq!(
        split_lines("a\n\nb\n"),
        vec![l("a", Lf), l("", Lf), l("b", Lf)]
    );
    assert_eq!(split_lines("\r\n\r\n"), vec![l("", CrLf), l("", CrLf)]);
}

// ── Terminator recognition ───────────────────────────────────────────────────

#[test]
fn crlf_is_one_terminator_not_cr_plus_lf() {
    assert_eq!(split_lines("a\r\nb"), vec![l("a", CrLf), l("b", NoEnd)]);
}

#[test]
fn lone_cr_terminates_a_line_like_in_java() {
    assert_eq!(split_lines("a\rb"), vec![l("a", Cr), l("b", NoEnd)]);
}

#[test]
fn lf_followed_by_cr_are_two_terminators() {
    assert_eq!(split_lines("a\n\r"), vec![l("a", Lf), l("", Cr)]);
}

#[test]
fn cr_followed_by_crlf_are_two_terminators() {
    assert_eq!(split_lines("a\r\r\n"), vec![l("a", Cr), l("", CrLf)]);
}

#[test]
fn mixed_endings_are_kept_per_line() {
    assert_eq!(
        split_lines("a\r\nb\rc\nd"),
        vec![l("a", CrLf), l("b", Cr), l("c", Lf), l("d", NoEnd)]
    );
}

// ── Content is opaque at this layer ──────────────────────────────────────────

#[test]
fn trailing_backslash_stays_in_the_content() {
    // Continuation is decided by a higher layer, not here.
    assert_eq!(
        split_lines("a=1 \\\n    b"),
        vec![l("a=1 \\", Lf), l("    b", NoEnd)]
    );
}

#[test]
fn whitespace_and_comments_are_not_interpreted() {
    assert_eq!(
        split_lines("  \t\n# c\n! d\n"),
        vec![l("  \t", Lf), l("# c", Lf), l("! d", Lf)]
    );
}

#[test]
fn non_ascii_content_is_preserved() {
    assert_eq!(split_lines("caf\u{e9}\n"), vec![l("caf\u{e9}", Lf)]);
}

#[test]
fn a_bom_is_not_stripped_here() {
    // BOM handling belongs to decoding, not to line splitting.
    assert_eq!(split_lines("\u{feff}a=1\n"), vec![l("\u{feff}a=1", Lf)]);
}

// ── Invariants ───────────────────────────────────────────────────────────────

/// All strings over a small alphabet up to length 6 (5461 strings), no extra dependency.
fn all_small_strings() -> Vec<String> {
    const ALPHABET: [char; 4] = ['a', '\r', '\n', '\\'];
    let mut all = vec![String::new()];
    let mut layer = vec![String::new()];
    for _ in 0..6 {
        layer = layer
            .iter()
            .flat_map(|s| ALPHABET.iter().map(move |c| format!("{s}{c}")))
            .collect();
        all.extend(layer.iter().cloned());
    }
    all
}

#[test]
fn join_inverts_split_for_every_small_string() {
    for text in all_small_strings() {
        assert_eq!(join_lines(&split_lines(&text)), text, "text = {text:?}");
    }
}

#[test]
fn only_the_last_line_may_lack_an_ending_and_content_has_no_terminators() {
    for text in all_small_strings() {
        let lines = split_lines(&text);
        for (i, line) in lines.iter().enumerate() {
            assert!(
                !line.content.contains(['\r', '\n']),
                "terminator inside content, text = {text:?}"
            );
            if line.ending == NoEnd {
                assert_eq!(i, lines.len() - 1, "text = {text:?}");
            }
        }
    }
}

// ── Real files ───────────────────────────────────────────────────────────────

/// Maps every byte to the char with the same value. Lossless for any bytes, and enough
/// here because all test files use an ASCII-compatible encoding.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

fn unlatin1(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap())
        .collect()
}

fn files_under(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

fn fixture_files() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut files = Vec::new();
    files_under(&root, &mut files);
    files
}

#[test]
fn every_fixture_file_survives_split_and_join_byte_for_byte() {
    let files = fixture_files();
    assert!(files.len() >= 10, "fixtures not found, got {files:?}");
    for path in files {
        let bytes = std::fs::read(&path).unwrap();
        let text = latin1(&bytes);
        let joined = join_lines(&split_lines(&text));
        assert_eq!(unlatin1(&joined), bytes, "{}", path.display());
    }
}

#[test]
fn mixed_endings_file_is_split_as_documented() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/edge/encodings/messages_pl.properties");
    let text = std::str::from_utf8(&std::fs::read(path).unwrap())
        .unwrap()
        .to_owned();
    let endings: Vec<LineEnding> = split_lines(&text).iter().map(|x| x.ending).collect();
    // comment LF, greeting CRLF, farewell CR, continued= LF, its continuation line CRLF
    assert_eq!(endings, vec![Lf, CrLf, Cr, Lf, CrLf]);
}

#[test]
fn file_without_final_newline_ends_with_no_ending() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/edge/encodings/messages_it.properties");
    let text = latin1(&std::fs::read(path).unwrap());
    let lines = split_lines(&text);
    assert_eq!(lines.last().map(|x| x.ending), Some(NoEnd));
}
