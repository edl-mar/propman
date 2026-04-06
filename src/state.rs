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
    /// How many dot-segments above the row's natural display the key-segment cursor
    /// has been extended with Ctrl+Left.  0 = show only the row's own display suffix.
    /// Resets to 0 on any row movement or filter change.
    pub key_segment_cursor: usize,
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
        self.key_segment_cursor = 0;
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
            key_segment_cursor: 0,
        }
    }

    /// The maximum value `key_segment_cursor` can reach for the current cursor row.
    /// Equal to `total_segments - 1` so the user can walk the anchor all the way
    /// up to the first segment, regardless of how many segments are already shown
    /// in the collapsed display.  Works for both Key and Header rows.
    pub fn key_seg_max(&self) -> usize {
        let full_key = match self.display_rows.get(self.cursor_row) {
            Some(DisplayRow::Key    { full_key, .. }) => full_key.as_str(),
            Some(DisplayRow::Header { prefix,   .. }) => prefix.as_str(),
            _ => return 0,
        };
        let real = full_key.find(':').map_or(full_key, |i| &full_key[i + 1..]);
        real.split('.').count().saturating_sub(1)
    }

    /// The "anchor" full_key the key-segment cursor points to.
    /// With `key_segment_cursor == 0` this is the row's own full_key / prefix.
    /// With `key_segment_cursor == k` it is the ancestor k levels above the row's key
    /// (i.e. the first `total - k` segments).  Works for both Key and Header rows.
    pub fn key_seg_anchor(&self) -> Option<String> {
        let full_key = match self.display_rows.get(self.cursor_row)? {
            DisplayRow::Key    { full_key, .. } => full_key.as_str(),
            DisplayRow::Header { prefix,   .. } => prefix.as_str(),
        };
        let k = self.key_segment_cursor;
        if k == 0 {
            return Some(full_key.to_string());
        }
        let colon      = full_key.find(':');
        let real       = colon.map_or(full_key, |i| &full_key[i + 1..]);
        let bundle_pfx = colon.map_or("", |i| &full_key[..=i]);
        let segs: Vec<&str> = real.split('.').collect();
        let take = segs.len().saturating_sub(k).max(1);
        Some(format!("{}{}", bundle_pfx, segs[..take].join(".")))
    }

    /// Return the full_key / prefix string for an arbitrary display row (not just the
    /// cursor row).  Used by sibling-navigation helpers.
    fn row_key_at(&self, idx: usize) -> &str {
        match self.display_rows.get(idx) {
            Some(DisplayRow::Key    { full_key, .. }) => full_key.as_str(),
            Some(DisplayRow::Header { prefix,   .. }) => prefix.as_str(),
            None => "",
        }
    }

    /// Find the row index of the nearest previous (forward=false) or next
    /// (forward=true) sibling at the current key-segment anchor level.
    ///
    /// "Sibling" means a row whose key shares the same parent prefix as the anchor
    /// but has a different immediate child segment.  For the forward direction we
    /// return the first such row; for the backward direction we return the topmost
    /// row of the nearest previous sibling subtree (so Up lands on the header/first
    /// entry of that group, not the last entry we happen to pass first).
    ///
    /// Returns `None` when no sibling exists in that direction.
    pub fn find_sibling_row(&self, forward: bool) -> Option<(usize, String)> {
        let anchor = self.key_seg_anchor()?;

        let colon = anchor.find(':');
        let real = colon.map_or(anchor.as_str(), |i| &anchor[i + 1..]);
        let bundle_pfx = colon.map_or("", |i| &anchor[..=i]);

        // Determine the sibling prefix and the anchor's own segment.
        // When the anchor is a top-level bundle key (no dot in real), the "parent"
        // is the bundle itself, so siblings share the bundle prefix.
        let (sibling_prefix, anchor_seg) = if let Some(dot_pos) = real.rfind('.') {
            let parent_real = &real[..dot_pos];
            let seg = &real[dot_pos + 1..];
            (format!("{bundle_pfx}{parent_real}."), seg.to_string())
        } else if !bundle_pfx.is_empty() {
            (bundle_pfx.to_string(), real.to_string())
        } else {
            return None; // no bundle and no dot — can't find siblings
        };

        let n = self.display_rows.len();
        let start = self.cursor_row;

        if forward {
            for i in (start + 1)..n {
                let rk = self.row_key_at(i);
                if rk.starts_with(&sibling_prefix) {
                    let seg = rk[sibling_prefix.len()..].split('.').next().unwrap_or("");
                    if seg != anchor_seg {
                        let sibling_anchor = format!("{sibling_prefix}{seg}");
                        return Some((i, sibling_anchor));
                    }
                }
            }
        } else {
            // Scan backward: collect the topmost row of the nearest previous sibling.
            let mut target_seg: Option<String> = None;
            let mut found: Option<usize> = None;

            for i in (0..start).rev() {
                let rk = self.row_key_at(i);
                if rk.starts_with(&sibling_prefix) {
                    let seg = rk[sibling_prefix.len()..].split('.').next().unwrap_or("").to_string();
                    if seg == anchor_seg {
                        continue; // still inside current sibling's subtree
                    }
                    match &target_seg {
                        None => { target_seg = Some(seg); found = Some(i); }
                        Some(ts) if *ts == seg => { found = Some(i); } // earlier row of same sibling
                        _ => break, // entered a different sibling — stop
                    }
                }
            }
            if let (Some(i), Some(seg)) = (found, target_seg) {
                let sibling_anchor = format!("{sibling_prefix}{seg}");
                return Some((i, sibling_anchor));
            }
        }

        None
    }

    /// Find the display row whose full_key / prefix exactly equals the current anchor,
    /// excluding the cursor row itself.  Used by Left navigation to jump the cursor
    /// to a parent row when it has a visible display entry.
    pub fn find_anchor_row(&self) -> Option<usize> {
        let anchor = self.key_seg_anchor()?;
        self.display_rows.iter().enumerate().find_map(|(i, row)| {
            if i == self.cursor_row { return None; }
            let rk = match row {
                DisplayRow::Key    { full_key, .. } => full_key.as_str(),
                DisplayRow::Header { prefix,   .. } => prefix.as_str(),
            };
            if rk == anchor { Some(i) } else { None }
        })
    }

    /// Find the first row that is a direct or indirect child of the current anchor.
    /// Searches the entire display list so it works even when the anchor's subtree
    /// starts before the cursor row (e.g. when k > 0).
    pub fn find_first_child(&self) -> Option<usize> {
        let anchor = self.key_seg_anchor()?;
        let child_prefix = format!("{anchor}.");
        self.display_rows.iter().position(|row| {
            let rk = match row {
                DisplayRow::Key    { full_key, .. } => full_key.as_str(),
                DisplayRow::Header { prefix,   .. } => prefix.as_str(),
            };
            rk.starts_with(&child_prefix)
        })
    }

    /// The dimmed prefix string to display before the row's natural display text
    /// when `key_segment_cursor > 0`.  Returns `None` when there is nothing extra to show.
    /// Works for both Key and Header rows.
    pub fn key_seg_extended_prefix(&self) -> Option<String> {
        let k = self.key_segment_cursor;
        if k == 0 { return None; }
        let (full_key, display) = match self.display_rows.get(self.cursor_row)? {
            DisplayRow::Key    { full_key, display, .. } => (full_key.as_str(), display.as_str()),
            DisplayRow::Header { prefix,   display, .. } => (prefix.as_str(),   display.as_str()),
        };
        let real = full_key.find(':').map_or(full_key, |i| &full_key[i + 1..]);
        let segs: Vec<&str> = real.split('.').collect();
        let total        = segs.len();
        let shown        = display.trim_start_matches('.').split('.').count();
        let k            = k.min(total.saturating_sub(shown));
        if k == 0 { return None; }
        let anchor_left  = total - shown - k;
        let context_left = total - shown;
        Some(segs[anchor_left..context_left].join("."))
    }
}
