use std::collections::HashSet;
use std::path::PathBuf;
use tui_textarea::TextArea;
use crate::{
    editor::CellEdit,
    filter,
    render_model::{self, DisplayRow},
    workspace::{self, Workspace},
};

/// A committed edit queued for the next Ctrl+S flush.
#[derive(Debug)]
pub enum PendingChange {
    /// Overwrite an existing key-value entry at `first_line..=last_line`.
    Update {
        path: PathBuf,
        first_line: usize,
        last_line: usize,
        key: String,   // bare key written to the file (no bundle prefix)
        value: String,
        full_key: String, // bundle-qualified key used for dirty tracking
    },
    /// Insert a brand-new key-value entry after `after_line`
    /// (0 means prepend before the first line).
    Insert {
        path: PathBuf,
        after_line: usize,
        key: String,   // bare key written to the file (no bundle prefix)
        value: String,
        full_key: String, // bundle-qualified key used for dirty tracking
    },
    /// Remove the key-value entry at `first_line..=last_line` from the file.
    Delete {
        path: PathBuf,
        first_line: usize,
        last_line: usize,
        full_key: String, // bundle-qualified key used for dirty tracking
    },
}

/// Scope of the current selection in Normal mode.
/// Tab cycles through the states; actions (rename, delete, pin) inherit it.
/// `ChildrenAll` (show hidden affected entries) is added with the pinning feature.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectionScope {
    /// Only the key under the cursor is in scope.
    Exact,
    /// The key and all filter-visible children are in scope.
    /// Hidden children are silently unaffected; a "+N ignored" hint comes later.
    Children,
    /// The key and ALL children (visible + hidden) are in scope.
    /// Hidden children are temp-pinned and surfaced in the table while this
    /// scope is active.
    ChildrenAll,
}

impl SelectionScope {
    pub fn cycle(&self) -> Self {
        match self {
            Self::Exact        => Self::Children,
            Self::Children     => Self::ChildrenAll,
            Self::ChildrenAll  => Self::Exact,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Exact        => "exact",
            Self::Children     => "+children",
            Self::ChildrenAll  => "+children all",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Mode {
    Normal,
    Editing,
    /// Sub-mode of Editing: the user just typed `\`. Enter inserts a newline
    /// instead of committing; any other key returns to Editing.
    Continuation,
    /// The user pressed `n`. The editor TextArea holds the new key name
    /// (pre-filled with the current prefix). Enter confirms creation.
    KeyNaming,
    /// The user pressed Enter on the key column (col 0). The editor TextArea
    /// holds the current key name for editing. Enter confirms the rename.
    /// Scope (exact / +children) is set in Normal mode before entering.
    KeyRenaming,
    /// The user pressed `d` on the key column (col 0). The editor TextArea
    /// shows the key name (read-only). Enter confirms deletion.
    /// Scope (exact / +children) is set in Normal mode before entering.
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
    /// Scope of the current selection; set by Tab in Normal mode and inherited
    /// by rename, delete, and pin actions.  Resets to Exact on row movement.
    pub selection_scope: SelectionScope,
    /// One-shot message shown in the status bar until the next keypress.
    /// Used for rename conflict errors and similar feedback.
    pub status_message: Option<String>,
    /// When true, a read-only preview pane is shown below the table in Normal
    /// and Filter modes.  The pane updates live as the cursor moves.
    /// Edit modes implicitly suppress it (they use the same pane slot).
    pub show_preview: bool,
    /// Bundle-qualified keys that have unsaved changes.  Derived from
    /// `pending_writes` after every save: a key is dirty iff it still has at
    /// least one entry in the pending queue.  Also populated immediately when a
    /// mutation is queued so the filter `#` sigil reflects in-flight changes.
    pub dirty_keys: HashSet<String>,
    /// Keys that are temporarily surfaced while `ChildrenAll` scope is active.
    /// Cleared on scope change, cancel, or commit.  Never written to disk.
    pub temp_pins: Vec<String>,
    /// Keys promoted to permanent pins after a `ChildrenAll` bulk op, or
    /// manually pinned by the user (`m`).  Bypasses the filter so they stay
    /// visible until explicitly unpinned.
    pub pinned_keys: HashSet<String>,
}

impl AppState {
    /// Returns the current value at the active locale cell, or `None` when on the
    /// key column or the key is absent from that locale file.
    pub fn current_cell_value(&self) -> Option<String> {
        if self.cursor_col == 0 {
            return None;
        }
        let locale_idx = self.cursor_col - 1;
        let full_key = match self.display_rows.get(self.cursor_row)? {
            DisplayRow::Key { full_key, .. } => full_key.as_str(),
            DisplayRow::Header { prefix, .. } => prefix.as_str(),
        };
        let locale = self.visible_locales.get(locale_idx)?;
        self.workspace.get_value(full_key, locale).map(|v| v.to_string())
    }

    /// Re-evaluates the filter query, rebuilds `display_rows` and `visible_locales`,
    /// then clamps the cursor to the new bounds.
    pub fn apply_filter(&mut self) {
        let query = self.filter_textarea.lines()[0].clone();
        let (filtered, visible) = if query.trim().is_empty() {
            (
                self.workspace.merged_keys.clone(),
                self.workspace.all_locales(),
            )
        } else {
            let expr = filter::parse(&query);
            let filtered = self.workspace.merged_keys.iter()
                .filter(|key| {
                    filter::evaluate(&expr, key, &self.workspace, &self.dirty_keys)
                        || self.temp_pins.contains(*key)
                        || self.pinned_keys.contains(*key)
                        || self.dirty_keys.contains(*key)
                })
                .cloned()
                .collect();
            let visible = filter::visible_locales(&expr, &self.workspace);
            (filtered, visible)
        };
        self.display_rows = render_model::build_display_rows(&filtered);
        self.visible_locales = visible;
        self.cursor_col = self.cursor_col.min(self.visible_locales.len());
        let max_row = self.display_rows.len().saturating_sub(1);
        self.cursor_row = self.cursor_row.min(max_row);
        self.clamp_cursor_col();
        self.clamp_scroll();
    }

    /// Returns the bundle name for the current cursor row, or `""` for bare keys.
    pub fn current_row_bundle(&self) -> &str {
        match self.display_rows.get(self.cursor_row) {
            Some(DisplayRow::Key { full_key, .. }) => workspace::split_key(full_key).0,
            Some(DisplayRow::Header { prefix, .. }) => workspace::split_key(prefix).0,
            None => "",
        }
    }

    /// Snaps `cursor_col` down to the nearest column available for the current
    /// row's bundle.  Stays at 0 (key column) as a safe fallback.
    pub fn clamp_cursor_col(&mut self) {
        if self.cursor_col == 0 {
            return;
        }
        let bundle = self.current_row_bundle().to_string();
        while self.cursor_col > 0 {
            let locale = &self.visible_locales[self.cursor_col - 1];
            if self.workspace.bundle_has_locale(&bundle, locale) {
                break;
            }
            self.cursor_col -= 1;
        }
    }

    /// Keeps `scroll_offset` in sync so the cursor stays visible.
    /// Uses a hardcoded viewport estimate; the renderer will clip naturally anyway.
    pub fn clamp_scroll(&mut self) {
        const VIEWPORT: usize = 20;
        if self.cursor_row < self.scroll_offset {
            self.scroll_offset = self.cursor_row;
        } else if self.cursor_row >= self.scroll_offset + VIEWPORT {
            self.scroll_offset = self.cursor_row - VIEWPORT + 1;
        }
    }

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
            selection_scope: SelectionScope::Exact,
            status_message: None,
            show_preview: false,
            temp_pins: Vec::new(),
            pinned_keys: HashSet::new(),
            dirty_keys: HashSet::new(),
        }
    }
}
