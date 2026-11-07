//! Specification of encoding detection, decoding and encoding (see `disk::encoding`).

use Encoding::{Latin1, Utf8, Utf8Bom, Windows1252};
use disk::encoding::{DecodeError, Decoded, Encoding, decode, encode};

fn detected(bytes: &[u8]) -> Decoded {
    decode(bytes, None).unwrap()
}

fn is(encoding: Encoding, text: &str) -> Decoded {
    Decoded {
        encoding,
        text: text.to_owned(),
    }
}

// ── Detection without a hint ─────────────────────────────────────────────────

#[test]
fn empty_input_is_utf8_with_empty_text() {
    assert_eq!(detected(b""), is(Utf8, ""));
}

#[test]
fn ascii_is_utf8() {
    assert_eq!(detected(b"a=1\n"), is(Utf8, "a=1\n"));
}

#[test]
fn valid_utf8_is_utf8() {
    assert_eq!(detected("caf\u{e9}".as_bytes()), is(Utf8, "caf\u{e9}"));
}

#[test]
fn valid_utf8_wins_even_if_it_looks_like_mojibake() {
    // C3 83 C2 A9 is valid UTF-8 for "Ã©"; it is never reinterpreted as Latin-1.
    assert_eq!(detected(b"\xc3\x83\xc2\xa9"), is(Utf8, "\u{c3}\u{a9}"));
}

#[test]
fn utf8_bom_is_detected_and_stripped_from_the_text() {
    assert_eq!(detected(b"\xef\xbb\xbfa=1"), is(Utf8Bom, "a=1"));
}

#[test]
fn bom_alone_is_utf8_bom_with_empty_text() {
    assert_eq!(detected(b"\xef\xbb\xbf"), is(Utf8Bom, ""));
}

#[test]
fn bom_followed_by_invalid_utf8_falls_back_and_keeps_the_bom_bytes_as_characters() {
    assert_eq!(
        detected(b"\xef\xbb\xbf\xe9"),
        is(Latin1, "\u{ef}\u{bb}\u{bf}\u{e9}")
    );
}

#[test]
fn invalid_utf8_without_c1_bytes_is_latin1() {
    assert_eq!(detected(b"caf\xe9"), is(Latin1, "caf\u{e9}"));
}

#[test]
fn invalid_utf8_with_bytes_in_0x80_to_0x9f_is_windows1252() {
    // 0x80 = euro sign, 0x93/0x94 = curly double quotes
    assert_eq!(
        detected(b"5 \x80 \x93x\x94 caf\xe9"),
        is(Windows1252, "5 \u{20ac} \u{201c}x\u{201d} caf\u{e9}")
    );
}

#[test]
fn an_undefined_windows1252_byte_forces_latin1() {
    // 0x81 is undefined in Windows-1252; the file is read as Latin-1 to stay lossless.
    assert_eq!(detected(b"\x93x\x81"), is(Latin1, "\u{93}x\u{81}"));
}

// ── Hint ─────────────────────────────────────────────────────────────────────

#[test]
fn a_hint_overrides_detection() {
    let bytes = b"\x93x\x94";
    assert_eq!(
        decode(bytes, Some(Latin1)).unwrap(),
        is(Latin1, "\u{93}x\u{94}")
    );
    assert_eq!(
        decode(bytes, Some(Windows1252)).unwrap(),
        is(Windows1252, "\u{201c}x\u{201d}")
    );
}

#[test]
fn latin1_hint_accepts_any_bytes() {
    for b in 0..=255u8 {
        assert!(decode(&[b], Some(Latin1)).is_ok(), "byte {b:#04x}");
    }
}

#[test]
fn utf8_hint_rejects_invalid_utf8() {
    assert_eq!(
        decode(b"caf\xe9", Some(Utf8)),
        Err(DecodeError::InvalidUtf8)
    );
    assert_eq!(
        decode(b"\xef\xbb\xbf\xe9", Some(Utf8Bom)),
        Err(DecodeError::InvalidUtf8)
    );
}

#[test]
fn utf8_hint_keeps_a_bom_as_a_character() {
    assert_eq!(
        decode(b"\xef\xbb\xbfa", Some(Utf8)).unwrap(),
        is(Utf8, "\u{feff}a")
    );
}

#[test]
fn windows1252_hint_rejects_undefined_bytes() {
    for b in [0x81u8, 0x8d, 0x8f, 0x90, 0x9d] {
        assert_eq!(
            decode(&[b], Some(Windows1252)),
            Err(DecodeError::UndefinedByte(b))
        );
    }
}

// ── Encoding ─────────────────────────────────────────────────────────────────

#[test]
fn utf8_encodes_plain_and_with_bom() {
    assert_eq!(encode("caf\u{e9}", Utf8), "caf\u{e9}".as_bytes());
    assert_eq!(encode("a", Utf8Bom), b"\xef\xbb\xbfa");
    assert_eq!(encode("", Utf8Bom), b"\xef\xbb\xbf");
}

#[test]
fn latin1_and_windows1252_encode_representable_characters_as_single_bytes() {
    assert_eq!(encode("caf\u{e9}", Latin1), b"caf\xe9");
    assert_eq!(encode("caf\u{e9}", Windows1252), b"caf\xe9");
    assert_eq!(encode("\u{20ac}\u{201c}", Windows1252), b"\x80\x93");
    assert_eq!(encode("\u{93}", Latin1), b"\x93");
}

#[test]
fn unrepresentable_characters_become_uppercase_unicode_escapes() {
    assert_eq!(encode("5 \u{20ac}", Latin1), b"5 \\u20AC");
    assert_eq!(encode("\u{436}", Windows1252), b"\\u0436");
    // U+0093 exists in Latin-1 but not in Windows-1252 (0x93 means a quote there).
    assert_eq!(encode("\u{93}", Windows1252), b"\\u0093");
}

#[test]
fn characters_outside_the_bmp_become_surrogate_pairs() {
    assert_eq!(encode("\u{1f600}", Latin1), b"\\uD83D\\uDE00");
}

#[test]
fn utf8_never_escapes() {
    assert_eq!(encode("\u{1f600}", Utf8), "\u{1f600}".as_bytes());
}

#[test]
fn existing_escapes_and_line_endings_are_left_alone() {
    assert_eq!(encode("a=\\u20ac\r\nb\n", Latin1), b"a=\\u20ac\r\nb\n");
}

// ── Invariant: saving an unchanged file restores the original bytes ──────────

fn assert_roundtrip(bytes: &[u8]) {
    let d = detected(bytes);
    assert_eq!(encode(&d.text, d.encoding), bytes, "bytes = {bytes:02x?}");
}

#[test]
fn every_single_byte_roundtrips() {
    for b in 0..=255u8 {
        assert_roundtrip(&[b]);
    }
}

#[test]
fn every_byte_pair_roundtrips() {
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            assert_roundtrip(&[a, b]);
        }
    }
}

#[test]
fn byte_sequences_over_interesting_bytes_roundtrip() {
    // BOM parts, UTF-8 lead/continuation bytes, C1 bytes (defined and undefined), ASCII.
    const BYTES: [u8; 11] = [
        0x41, 0x5c, 0x0d, 0x0a, 0xef, 0xbb, 0xbf, 0xc3, 0x81, 0x93, 0xe9,
    ];
    let mut seqs: Vec<Vec<u8>> = vec![vec![]];
    for _ in 0..5 {
        seqs = seqs
            .iter()
            .flat_map(|s| {
                BYTES.iter().map(move |&b| {
                    let mut t = s.clone();
                    t.push(b);
                    t
                })
            })
            .collect();
        for s in &seqs {
            assert_roundtrip(s);
        }
    }
}

// ── Real files ───────────────────────────────────────────────────────────────

fn fixture(rel: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(rel),
    )
    .unwrap()
}

#[test]
fn the_encoding_fixtures_are_detected_as_intended() {
    let enc = |name: &str| detected(&fixture(&format!("edge/encodings/{name}"))).encoding;
    assert_eq!(enc("messages.properties"), Utf8); // pure ASCII
    assert_eq!(enc("messages_de.properties"), Utf8Bom);
    assert_eq!(enc("messages_fr.properties"), Latin1);
    assert_eq!(enc("messages_it.properties"), Windows1252);
    assert_eq!(enc("messages_pl.properties"), Utf8);
}

#[test]
fn the_encoding_fixtures_decode_to_the_expected_text() {
    let text = |name: &str| detected(&fixture(&format!("edge/encodings/{name}"))).text;
    assert!(text("messages_de.properties").starts_with("# UTF-8 with BOM"));
    assert!(text("messages_fr.properties").contains("\u{c0} bient\u{f4}t"));
    assert!(text("messages_it.properties").contains("Prezzo: 5 \u{20ac}"));
    assert!(text("messages_pl.properties").contains("Cze\u{15b}\u{107}"));
}

#[test]
fn every_fixture_file_roundtrips_byte_for_byte() {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures"),
        &mut files,
    );
    assert!(files.len() >= 10, "fixtures not found, got {files:?}");
    for path in files {
        assert_roundtrip(&std::fs::read(&path).unwrap());
    }
}
