use ratatui::style::Style;
use tui_textarea::TextArea;

/// In-progress text edit for a single translation cell.
/// Owned by AppState while Mode::Editing is active; dropped on commit or cancel.
///
/// `TextArea` manages cursor position and line editing.
/// `original` is kept separately so we can detect whether the value was modified.
#[derive(Debug)]
pub struct CellEdit {
    pub original: String,
    pub textarea: TextArea<'static>,
}

impl CellEdit {
    pub fn new(value: String) -> Self {
        let mut textarea = TextArea::from([value.clone()]);
        // Place the cursor at the end of the line so typing appends naturally.
        textarea.move_cursor(tui_textarea::CursorMove::End);
        // Don't highlight the current line — it looks noisy in a single-line editor pane.
        textarea.set_cursor_line_style(Style::default());
        Self { original: value, textarea }
    }

    /// The current value of the cell.
    /// Lines are joined with no separator — continuation lines are a file-format
    /// detail; the logical value is always a single string. The user is responsible
    /// for any trailing space before the `\` continuation marker.
    pub fn current_value(&self) -> String {
        self.textarea.lines().join("")
    }

    pub fn is_modified(&self) -> bool {
        self.current_value() != self.original
    }
}
