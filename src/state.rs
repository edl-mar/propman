use std::path::PathBuf;
use tui_textarea::TextArea;
use crate::{
    editor::CellEdit,
    render_model::{self, DisplayRow},
    workspace::Workspace,
};

/// A committed edit queued for the next Ctrl+S flush.
#[derive(Debug)]
pub enum PendingChange {
    /// Overwrite an existing key-value entry at `first_line..=last_line`.
    Update {
        path: PathBuf,
        first_line: usize,
        last_line: usize,
        key: String,
        value: String,
    },
    /// Insert a brand-new key-value entry after `after_line`
    /// (0 means prepend before the first line).
    Insert {
        path: PathBuf,
        after_line: usize,
        key: String,
        value: String,
    },
    /// Remove the key-value entry at `first_line..=last_line` from the file.
    Delete {
        path: PathBuf,
        first_line: usize,
        last_line: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    Editing,
    /// Sub-mode of Editing: the user just typed `\`. Enter inserts a newline
    /// instead of committing; any other key returns to Editing.
    Continuation,
    /// The user pressed Enter on a Header row. The editor TextArea holds the
    /// new key name (pre-filled with the header prefix + `.`). Enter confirms
    /// the name and transitions straight to value Editing for that key.
    KeyNaming,
    /// The user pressed Enter on the key column (col 0). The editor TextArea
    /// holds the current key name for editing. Enter confirms the rename;
    /// Tab toggles between renaming the exact key vs. the whole prefix subtree
    /// (only shown/active when the key has children).
    KeyRenaming,
    /// The user pressed `d` on the key column (col 0). The editor TextArea
    /// shows the key name (read-only). Enter confirms deletion; Tab toggles
    /// between deleting the exact key vs. the whole prefix subtree.
    Deleting,
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
    /// Edits committed but not yet flushed to disk. Flushed by `Message::SaveFile`.
    pub pending_writes: Vec<PendingChange>,
    /// Set to true by `Message::Quit`; the TUI loop exits on the next iteration.
    pub quitting: bool,
    /// Active cell editor; present while mode is Editing, KeyNaming, or KeyRenaming.
    pub edit_buffer: Option<CellEdit>,
    /// Whether the active key-rename should also rename all keys sharing the
    /// same prefix (i.e. children in the trie). Only meaningful in KeyRenaming.
    pub rename_children: bool,
    /// Whether the active key-deletion should also delete all keys sharing the
    /// same prefix. Only meaningful in Deleting mode.
    pub delete_children: bool,
    /// One-shot message shown in the status bar until the next keypress.
    /// Used for rename conflict errors and similar feedback.
    pub status_message: Option<String>,
    /// When true, a read-only preview pane is shown below the table in Normal
    /// and Filter modes.  The pane updates live as the cursor moves.
    /// Edit modes implicitly suppress it (they use the same pane slot).
    pub show_preview: bool,
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
            rename_children: false,
            delete_children: false,
            status_message: None,
            show_preview: false,
        }
    }
}
