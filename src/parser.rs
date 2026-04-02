use std::{
    fs,
    io::{self, BufRead},
    path::Path,
};

/// A single parsed entry from a .properties file.
/// Line numbers are 1-based and preserved so the writer can target them exactly.
///
/// `KeyValue` spans `first_line..=last_line` to support `\`-continuation lines.
/// For single-line values `first_line == last_line`.
/// `Comment` and `Blank` are always exactly one line.
#[derive(Debug, Clone)]
pub enum FileEntry {
    KeyValue { first_line: usize, last_line: usize, key: String, value: String },
    Comment  { line: usize, raw: String },
    Blank    { line: usize },
}

pub fn parse(path: &Path) -> io::Result<Vec<FileEntry>> {
    let file = fs::File::open(path)?;
    // Collect upfront so we can look ahead for continuation lines.
    let raw_lines: Vec<String> = io::BufReader::new(file)
        .lines()
        .collect::<io::Result<_>>()?;

    let mut entries = Vec::new();
    let mut i = 0;

    while i < raw_lines.len() {
        let first_line = i + 1; // 1-based
        let raw = &raw_lines[i];
        let trimmed = raw.trim();

        if trimmed.is_empty() {
            entries.push(FileEntry::Blank { line: first_line });
            i += 1;
        } else if trimmed.starts_with('#') || trimmed.starts_with('!') {
            entries.push(FileEntry::Comment { line: first_line, raw: raw.clone() });
            i += 1;
        } else if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim().to_string();
            let mut value = v.trim_start().to_string();
            i += 1;

            // Join any \-terminated continuation lines.
            while value.ends_with('\\') {
                value.pop(); // strip trailing backslash
                if i < raw_lines.len() {
                    // Leading whitespace on continuation lines is intentionally stripped.
                    value.push_str(raw_lines[i].trim());
                    i += 1;
                } else {
                    break;
                }
            }

            // i now points past the last consumed line, so last line is i (1-based).
            let last_line = i;
            entries.push(FileEntry::KeyValue { first_line, last_line, key, value });
        } else {
            // Lines without '=' (malformed or bare continuation) — preserve verbatim.
            entries.push(FileEntry::Comment { line: first_line, raw: raw.clone() });
            i += 1;
        }
    }

    Ok(entries)
}
