use std::collections::HashSet;
use std::path::PathBuf;
use tui_textarea::TextArea;
use crate::{
    editor::CellEdit,
    filter::{self, ColumnDirective},
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
    /// The user pressed `n` on a bundle-level header. The editor TextArea holds
    /// the locale name being typed. Enter creates a new empty locale file.
    LocaleNaming,
    /// The user pressed `N`. The editor TextArea holds the bundle name being typed.
    /// Enter creates a new empty `{name}_{first_locale}.properties` file.
    BundleNaming,
    /// The user pressed `p`. Clipboard history is shown as a horizontal panel;
    /// normal navigation moves the table cursor to choose a destination.
    Pasting,
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
    /// Column visibility directive derived from `:?` / `:!` filter terms.
    /// Applied per-row in the table renderer to hide present/missing cells.
    pub column_directive: ColumnDirective,
    /// Per-locale yank history.  Newest entry first; capped at 10 per locale.
    /// `p` opens paste mode where entries are reviewed and applied.
    pub clipboard: std::collections::HashMap<String, Vec<String>>,
    /// The most recently yanked value, used for Ctrl+P quick-paste.
    pub clipboard_last: Option<String>,
    /// Index into the sorted clipboard locales; the focused column in paste mode.
    pub paste_locale_cursor: usize,
    /// Per-locale position in the history list (the `>` marker in paste mode).
    pub paste_history_pos: std::collections::HashMap<String, usize>,
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

    /// Derives the set of locales that have at least one pending (unsaved) write.
    /// Used to compute which locale columns to surface when `:#` or `#` is in the filter.
    fn compute_dirty_locales(&self) -> HashSet<String> {
        // Build a path → locale map from the workspace.
        let path_to_locale: std::collections::HashMap<&std::path::Path, &str> = self
            .workspace
            .groups
            .iter()
            .flat_map(|g| g.files.iter())
            .map(|f| (f.path.as_path(), f.locale.as_str()))
            .collect();

        self.pending_writes
            .iter()
            .filter_map(|c| {
                let path = match c {
                    PendingChange::Update { path, .. } => path.as_path(),
                    PendingChange::Insert { path, .. } => path.as_path(),
                    PendingChange::Delete { path, .. } => path.as_path(),
                };
                path_to_locale.get(path).map(|locale| locale.to_string())
            })
            .collect()
    }

    /// Re-evaluates the filter query, rebuilds `display_rows` and `visible_locales`,
    /// then clamps the cursor to the new bounds.
    pub fn apply_filter(&mut self) {
        // Remember the selected key so we can restore cursor position after rebuild.
        let selected_key: Option<String> = match self.display_rows.get(self.cursor_row) {
            Some(DisplayRow::Key    { full_key, .. }) => Some(full_key.clone()),
            Some(DisplayRow::Header { prefix,   .. }) => Some(prefix.clone()),
            None => None,
        };

        let query = self.filter_textarea.lines()[0].clone();
        let (filtered, visible, directive) = if query.trim().is_empty() {
            (
                self.workspace.merged_keys.clone(),
                self.workspace.all_locales(),
                ColumnDirective::None,
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
            let dirty_locales = self.compute_dirty_locales();
            let visible = filter::visible_locales(&expr, &self.workspace, &dirty_locales);
            let directive = filter::column_directive(&expr);
            (filtered, visible, directive)
        };
        let bundle_names = self.workspace.bundle_names();
        self.display_rows = render_model::build_display_rows(&filtered, &bundle_names);
        self.visible_locales = visible;
        self.column_directive = directive;
        self.cursor_col = self.cursor_col.min(self.visible_locales.len());
        let max_row = self.display_rows.len().saturating_sub(1);
        // Restore cursor to the same key if still visible; otherwise clamp.
        self.cursor_row = selected_key
            .and_then(|key| self.display_rows.iter().position(|r| match r {
                DisplayRow::Key    { full_key, .. } => *full_key == key,
                DisplayRow::Header { prefix,   .. } => *prefix   == key,
            }))
            .unwrap_or_else(|| self.cursor_row.min(max_row));
        self.clamp_cursor_col();
        self.clamp_scroll();
    }

    /// Returns clipboard locales in table column order (`visible_locales` first,
    /// then any clipboard locales not currently visible, alphabetically).
    pub fn paste_locales(&self) -> Vec<String> {
        let visible_set: std::collections::HashSet<&str> =
            self.visible_locales.iter().map(|s| s.as_str()).collect();
        let mut keys: Vec<String> = self.visible_locales.iter()
            .filter(|l| self.clipboard.contains_key(*l))
            .cloned()
            .collect();
        let mut extra: Vec<String> = self.clipboard.keys()
            .filter(|l| !visible_set.contains(l.as_str()))
            .cloned()
            .collect();
        extra.sort();
        keys.extend(extra);
        keys
    }

    /// Returns the bundle name for the current cursor row, or `""` for bare keys.
    pub fn current_row_bundle(&self) -> &str {
        match self.display_rows.get(self.cursor_row) {
            Some(DisplayRow::Key { full_key, .. }) => workspace::split_key(full_key).0,
            Some(DisplayRow::Header { prefix, .. }) => {
                // Bundle-level headers have no colon — the prefix IS the bundle name.
                if self.workspace.is_bundle_name(prefix) {
                    prefix.as_str()
                } else {
                    workspace::split_key(prefix).0
                }
            }
            None => "",
        }
    }

    /// Snaps `cursor_col` down to the nearest column available for the current
    /// row's bundle.  Stays at 0 (key column) as a safe fallback.
    pub fn clamp_cursor_col(&mut self) {
        if self.cursor_col == 0 {
            return;
        }
        // `current_row_bundle()` now returns the real bundle name for bundle-level
        // headers too, so the loop below handles them correctly without a special case.
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
        let bundle_names = workspace.bundle_names();
        let display_rows = render_model::build_display_rows(&workspace.merged_keys, &bundle_names);
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
            column_directive: ColumnDirective::None,
            clipboard: std::collections::HashMap::new(),
            clipboard_last: None,
            paste_locale_cursor: 0,
            paste_history_pos: std::collections::HashMap::new(),
        }
    }
}
