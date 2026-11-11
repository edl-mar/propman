//! The preview of the file under the cursor in the overview, derived from the state each time it is
//! needed and made of plain values, so that a view only draws.
//!
//! The preview shows the raw lines of the file. It has a window: the lines from the scroll
//! position on, as many as the preview has rows (the height of the list minus the header row).
//! A text is shown as it is, except that control characters (tabs included) become `·`; the line
//! ending is written as text: `LF`, `CRLF`, `CR`, and `(none)` for a last line without one.

use types::{Encoding, LineEnding};

use crate::overview::{OverviewItem, overview};
use crate::state::{AppState, View};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// The name of the file, without its folder.
    pub name: String,
    pub encoding: Encoding,
    /// The number of lines of the whole file.
    pub total: usize,
    /// The index of the first line of the window (0 for the first line of the file).
    pub first: usize,
    pub lines: Vec<PreviewLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewLine {
    /// The number of the line in the file, counted from 1.
    pub number: usize,
    pub text: String,
    pub ending: &'static str,
}

/// The preview of the item under the cursor: `None` unless it is a file.
pub fn preview(state: &AppState) -> Option<Preview> {
    let View::Overview(view) = state.view();
    let items = overview(state.session());
    let Some(OverviewItem::File {
        id, name, encoding, ..
    }) = items.get(view.index())
    else {
        return None;
    };
    let all = state.session().store().file_lines(*id);
    let first = view.preview_scroll();
    let lines = all
        .iter()
        .enumerate()
        .skip(first)
        .take(view.preview_rows())
        .map(|(index, line)| PreviewLine {
            number: index + 1,
            text: line
                .content
                .chars()
                .map(|c| if c.is_control() { '\u{b7}' } else { c })
                .collect(),
            ending: ending_text(line.ending),
        })
        .collect();
    Some(Preview {
        name: name.clone(),
        encoding: *encoding,
        total: all.len(),
        first,
        lines,
    })
}

fn ending_text(ending: LineEnding) -> &'static str {
    match ending {
        LineEnding::Lf => "LF",
        LineEnding::CrLf => "CRLF",
        LineEnding::Cr => "CR",
        LineEnding::None => "(none)",
    }
}
