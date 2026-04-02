use std::path::PathBuf;
use tui_textarea::TextArea;
use crate::{
    editor::CellEdit,
    render_model::{self, DisplayRow},
    workspace::Workspace,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    Editing,
    /// Sub-mode of Editing: the user just typed `\`. Enter inserts a newline
    /// instead of committing; any other key returns to Editing.
    Continuation,
    Filter,
}

#[derive(Debug)]
pub struct AppState {
    pub workspace: Workspace,
    /// Flat list of header + key rows derived from `workspace.merged_keys`.
    /// `cursor_row` indexes into this vec; `Header` rows are skipped during navigation.
    pub display_rows: Vec<DisplayRow>,
    /// Locale columns currently visible; a subset of `workspace.all_locales()`
    /// when a locale selector is active, otherwise the full list.
    pub visible_locales: Vec<String>,
    /// Index into `display_rows`. Always points at a `Key` variant.
    pub cursor_row: usize,
    /// Index into `visible_locales`.
    pub cursor_col: usize,
    pub scroll_offset: usize,
    pub mode: Mode,
    /// Always-present single-line TextArea backing the filter bar.
    /// The current query is `filter_textarea.lines()[0]`.
    pub filter_textarea: TextArea<'static>,
    pub unsaved_changes: bool,
    /// Edits committed but not yet written to disk.
    /// Each entry is `(file_path, first_line, last_line, key, new_value)`.
    /// `first_line..=last_line` is the range occupied by the original value
    /// (continuation lines included). Flushed by `Message::SaveFile`.
    pub pending_writes: Vec<(PathBuf, usize, usize, String, String)>,
    /// Set to true by `Message::Quit`; the TUI loop exits on the next iteration.
    pub quitting: bool,
    /// Active cell editor; present only while `mode == Mode::Editing`.
    pub edit_buffer: Option<CellEdit>,
}

impl AppState {
    pub fn new(workspace: Workspace) -> Self {
        let display_rows = render_model::build_display_rows(&workspace.merged_keys);
        let visible_locales = workspace.all_locales();
        let cursor_row = 0;
        Self {
            workspace,
            display_rows,
            visible_locales,
            cursor_row,
            cursor_col: 0,
            scroll_offset: 0,
            mode: Mode::Normal,
            filter_textarea: TextArea::default(),
            unsaved_changes: false,
            pending_writes: Vec::new(),
            quitting: false,
            edit_buffer: None,
        }
    }
}
