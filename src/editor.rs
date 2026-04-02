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
        // Split on '\n' so that values previously saved with continuation lines
        // are re-opened as multi-line in the editor.
        let lines: Vec<&str> = value.split('\n').collect();
        let mut textarea = TextArea::from(lines);
        // Place the cursor at the end of the last line so typing appends naturally.
        textarea.move_cursor(tui_textarea::CursorMove::End);
        // Don't highlight the current line — it looks noisy in a single-line editor pane.
        textarea.set_cursor_line_style(Style::default());
        Self { original: value, textarea }
    }

    /// The physical value as it will be written to disk.
    /// Lines are joined with '\n'; a line that ends with '\' becomes a
    /// continuation line in the .properties file format.
    pub fn current_value(&self) -> String {
        self.textarea.lines().join("\n")
    }

    pub fn is_modified(&self) -> bool {
        self.current_value() != self.original
    }
}
