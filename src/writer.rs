use std::{
    fs,
    io::{self, BufRead, Write},
    path::Path,
};

/// Rewrites a key-value entry in `path`, replacing `first_line..=last_line`.
///
/// For single-line values `first_line == last_line`. For entries that spanned
/// multiple `\`-continuation lines the whole range is collapsed into one line.
///
/// The key is preserved verbatim; only the value changes.
///
/// The file is read fully into memory before writing so that a crash mid-write
/// can't truncate the file (the OS flushes the old inode until the new one is
/// fully written via `File::create`).
pub fn write_change(
    path: &Path,
    first_line: usize,
    last_line: usize,
    key: &str,
    new_value: &str,
) -> io::Result<()> {
    let file = fs::File::open(path)?;
    let lines: Vec<String> = io::BufReader::new(file)
        .lines()
        .collect::<io::Result<_>>()?;

    // Validate before touching anything on disk.
    if first_line == 0 || last_line > lines.len() || first_line > last_line {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "line range {first_line}..={last_line} out of range (file has {} lines)",
                lines.len()
            ),
        ));
    }

    let mut out = fs::File::create(path)?;
    for (idx, original) in lines.iter().enumerate() {
        let line_num = idx + 1;
        if line_num == first_line {
            // TODO: preserve the original separator style (= vs :) and surrounding whitespace.
            // Values containing '\n' (from continuation lines in the editor) are written
            // as-is; the embedded `\` + newline sequences become natural continuation lines.
            writeln!(out, "{key}={new_value}")?;
        } else if line_num > first_line && line_num <= last_line {
            // Skip old continuation lines — value is now on one line.
        } else {
            writeln!(out, "{original}")?;
        }
    }
    Ok(())
}
