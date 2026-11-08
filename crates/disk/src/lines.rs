//! Lossless split of decoded text into physical lines.
//!
//! Invariant: `join_lines(&split_lines(text)) == text` for every `text`.
//! Only the last line may have `LineEnding::None`. Content never contains `\r` or `\n`.
//! Knows nothing about keys, values, continuation lines, encodings or BOMs.

pub use types::LineEnding;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Line<'a> {
    pub content: &'a str,
    pub ending: LineEnding,
}

pub fn split_lines(text: &str) -> Vec<Line<'_>> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut i = 0;
    // `\r` and `\n` are ASCII, so every index we slice at is a char boundary.
    while i < bytes.len() {
        let (ending, width) = match bytes[i] {
            b'\n' => (LineEnding::Lf, 1),
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => (LineEnding::CrLf, 2),
            b'\r' => (LineEnding::Cr, 1),
            _ => {
                i += 1;
                continue;
            }
        };
        lines.push(Line {
            content: &text[start..i],
            ending,
        });
        i += width;
        start = i;
    }
    if start < bytes.len() {
        lines.push(Line {
            content: &text[start..],
            ending: LineEnding::None,
        });
    }
    lines
}

pub fn join_lines(lines: &[Line<'_>]) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(line.content);
        out.push_str(line.ending.as_str());
    }
    out
}
