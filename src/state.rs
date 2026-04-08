use std::collections::HashSet;
use std::path::PathBuf;
use tui_textarea::TextArea;
use crate::{
    editor::CellEdit,
    filter::{self, ColumnDirective},
    render_model::{self, DisplayRow, RenderModel},
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

/// Which column region the cursor is in.
///
/// Replaces the old `(cursor_col: usize, key_segment_cursor: usize)` pair.
/// `cursor_col == 0` → `Key { segment: 0 }`; `cursor_col == n+1` → `Locale(n)`.
/// The `segment` field tracks how many dot-segments above the row's leaf the
/// key-segment anchor has been extended (0 = anchor at the full key / leaf).
#[derive(Debug, Clone, PartialEq)]
pub enum CursorSection {
    Key { segment: usize },
    Locale(usize),
}

impl CursorSection {
    /// Returns `true` when the cursor is in the key column.
    pub fn is_key(&self) -> bool {
        matches!(self, Self::Key { .. })
    }

    /// Returns the 0-based locale index, or `None` when on the key column.
    pub fn locale_idx(&self) -> Option<usize> {
        match self {
            Self::Locale(idx) => Some(*idx),
            Self::Key { .. } => None,
        }
    }

    /// Resets the key-segment anchor to the leaf position (segment = 0).
    /// A no-op when the cursor is already on a locale column.
    pub fn reset_segment(self) -> Self {
        match self {
            Self::Key { .. } => Self::Key { segment: 0 },
            other => other,
        }
    }
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
    /// Hierarchical render model built in parallel with `display_rows`.
    /// The renderer will switch to consuming this once Step 2 of the refactoring lands.
    pub render_model: RenderModel,
    /// Locale columns currently visible; a subset of `workspace.all_locales()`
    /// when a locale selector is active, otherwise the full list.
    pub visible_locales: Vec<String>,
    /// Index into `display_rows`. Always points at a `Key` variant.
    pub cursor_row: usize,
    /// Which column region the cursor occupies.
    pub cursor_section: CursorSection,
    /// The locale the user last explicitly navigated to.  `clamp_cursor_section`
    /// will restore this locale whenever the bundle at the new row has it,
    /// giving a "sticky column" effect across bundles that lack some locales.
    /// Set on every explicit Left/Right navigation that lands on a locale.
    /// Never set by clamping — only by intentional user movement.
    pub preferred_locale: Option<String>,
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
        let locale_idx = self.cursor_section.locale_idx()?;
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

    /// Derives the set of `(full_key, locale)` pairs that have at least one pending
    /// (unsaved) write.  Used to compute `LocaleCell.is_dirty` in the render model.
    fn compute_dirty_cells(&self) -> HashSet<(String, String)> {
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
                let (path, full_key) = match c {
                    PendingChange::Update { path, full_key, .. } => {
                        (path.as_path(), full_key.as_str())
                    }
                    PendingChange::Insert { path, full_key, .. } => {
                        (path.as_path(), full_key.as_str())
                    }
                    PendingChange::Delete { path, full_key, .. } => {
                        (path.as_path(), full_key.as_str())
                    }
                };
                path_to_locale
                    .get(path)
                    .map(|locale| (full_key.to_string(), locale.to_string()))
            })
            .collect()
    }

    /// Re-evaluates the filter query, rebuilds `display_rows` and `visible_locales`,
    /// then clamps the cursor to the new bounds.
    pub fn apply_filter(&mut self) {
        self.cursor_section = self.cursor_section.clone().reset_segment();
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
        let dirty_cells = self.compute_dirty_cells();
        let new_rm = render_model::build_render_model(
            &self.workspace,
            &filtered,
            &bundle_names,
            &visible,
            &self.dirty_keys,
            &dirty_cells,
            &self.pinned_keys,
            &self.temp_pins,
        );
        self.display_rows = render_model::display_rows_from_render_model(&new_rm);
        self.render_model = new_rm;
        self.visible_locales = visible;
        self.column_directive = directive;
        let max_row = self.display_rows.len().saturating_sub(1);
        // Restore cursor to the same key if still visible; otherwise clamp.
        self.cursor_row = selected_key
            .and_then(|key| self.display_rows.iter().position(|r| match r {
                DisplayRow::Key    { full_key, .. } => *full_key == key,
                DisplayRow::Header { prefix,   .. } => *prefix   == key,
            }))
            .unwrap_or_else(|| self.cursor_row.min(max_row));
        self.clamp_cursor_section();
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

    /// Move the cursor to a locale column and record it as the preferred locale.
    /// Always call this for explicit user-initiated locale navigation so that
    /// `clamp_cursor_section` can restore the preference on rows that lack the
    /// current locale.  Do NOT call it from clamping code — only intentional
    /// movement should update the preference.
    pub fn set_locale_cursor(&mut self, idx: usize) {
        self.cursor_section = CursorSection::Locale(idx);
        self.preferred_locale = self.visible_locales.get(idx).cloned();
    }

    /// Snaps `cursor_section` to the best available locale column for the current
    /// row's bundle, or falls back to `Key { segment: 0 }`.
    ///
    /// Priority: preferred locale > nearest locale to the left > key column.
    /// Never modifies `preferred_locale` — only explicit user navigation does that.
    pub fn clamp_cursor_section(&mut self) {
        let idx = match self.cursor_section {
            CursorSection::Locale(idx) => idx,
            CursorSection::Key { .. } => return,
        };
        if self.visible_locales.is_empty() {
            self.cursor_section = CursorSection::Key { segment: 0 };
            return;
        }
        let bundle = self.current_row_bundle().to_string();

        // First priority: restore the preferred locale if this bundle has it.
        if let Some(ref preferred) = self.preferred_locale.clone() {
            if let Some(pref_idx) = self.visible_locales.iter().position(|l| l == preferred) {
                if self.workspace.bundle_has_locale(&bundle, &self.visible_locales[pref_idx]) {
                    self.cursor_section = CursorSection::Locale(pref_idx);
                    return;
                }
            }
        }

        // Fall back: snap left to the nearest available locale.
        let mut i = idx.min(self.visible_locales.len() - 1);
        loop {
            if self.workspace.bundle_has_locale(&bundle, &self.visible_locales[i]) {
                self.cursor_section = CursorSection::Locale(i);
                return;
            }
            if i == 0 {
                break;
            }
            i -= 1;
        }
        self.cursor_section = CursorSection::Key { segment: 0 };
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
        let visible_locales = workspace.all_locales();
        let empty_keys: HashSet<String> = HashSet::new();
        let empty_cells: HashSet<(String, String)> = HashSet::new();
        let hier_model = render_model::build_render_model(
            &workspace,
            &workspace.merged_keys,
            &bundle_names,
            &visible_locales,
            &empty_keys,
            &empty_cells,
            &empty_keys,
            &[],
        );
        let display_rows = render_model::display_rows_from_render_model(&hier_model);
        let cursor_row = 0;
        Self {
            workspace,
            display_rows,
            render_model: hier_model,
            visible_locales,
            cursor_row,
            cursor_section: CursorSection::Key { segment: 0 },
            preferred_locale: None,
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
        let k = match self.cursor_section {
            CursorSection::Key { segment } => segment,
            CursorSection::Locale(_) => 0,
        };
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

    /// Find the nearest row in direction `forward` at the same absolute segment
    /// depth as the current anchor.  "Absolute depth" is the number of dot-segments
    /// in the bundle-qualified key counted from the left (after the `:`), so
    /// `messages:app.title` has depth 2.  The row just has to share that depth —
    /// it doesn't need to share a parent prefix (cousins qualify).
    ///
    /// If no row exists at the current depth in that direction, we reduce depth by
    /// 1 and retry, continuing until depth 1 is exhausted.
    ///
    /// For the backward direction the topmost row of the nearest same-depth group
    /// is returned (landing on the header/first entry, not the last one passed).
    ///
    /// Returns `(row_index, k)` where `k` is the new `segment` offset to set so the
    /// anchor highlights the correct depth level on the target row.  Returns `None`
    /// when nothing navigable is found at any depth.
    pub fn find_depth_neighbor(&self, forward: bool) -> Option<(usize, usize)> {
        let anchor = self.key_seg_anchor()?;
        let colon = anchor.find(':');
        let bundle_pfx = colon.map_or("", |i| &anchor[..=i]);
        let anchor_real = colon.map_or(anchor.as_str(), |i| &anchor[i + 1..]);
        let anchor_segs: Vec<&str> = anchor_real.split('.').collect();
        let max_depth = anchor_segs.len();
        let n = self.display_rows.len();
        let start = self.cursor_row;

        // The d-depth prefix of the anchor (first d segments joined with '.').
        // Used to skip rows that are inside the same subtree.
        let anchor_prefix = |d: usize| -> String { anchor_segs[..d].join(".") };

        // Given a row index, return `Some((d-depth prefix string, total_segs))` if the
        // row belongs to the same bundle and has at least d segments, else `None`.
        let row_info_at = |idx: usize, d: usize| -> Option<(String, usize)> {
            let rk = self.row_key_at(idx);
            let rk_colon = rk.find(':');
            if rk_colon.map_or("", |j| &rk[..=j]) != bundle_pfx {
                return None;
            }
            let rk_real = rk_colon.map_or(rk, |j| &rk[j + 1..]);
            let segs: Vec<&str> = rk_real.split('.').collect();
            if segs.len() < d {
                return None;
            }
            Some((segs[..d].join("."), segs.len()))
        };

        for depth in (1..=max_depth).rev() {
            let ap = anchor_prefix(depth);

            if forward {
                for i in (start + 1)..n {
                    if let Some((prefix_d, total_segs)) = row_info_at(i, depth) {
                        if prefix_d != ap {
                            // k = how many segments below the depth-anchor the target row goes
                            let k = total_segs.saturating_sub(depth);
                            return Some((i, k));
                        }
                    }
                }
            } else {
                // Backward: we want the topmost row of the nearest previous group at
                // this depth that has a different d-prefix than the anchor.
                let mut found_prefix: Option<String> = None;
                let mut topmost: Option<(usize, usize)> = None;

                for i in (0..start).rev() {
                    if let Some((prefix_d, total_segs)) = row_info_at(i, depth) {
                        match &found_prefix {
                            None => {
                                if prefix_d != ap {
                                    found_prefix = Some(prefix_d);
                                    topmost = Some((i, total_segs));
                                }
                            }
                            Some(fp) if *fp == prefix_d => {
                                // Earlier row of the same neighbor group — keep going up.
                                topmost = Some((i, total_segs));
                            }
                            _ => break, // crossed into yet another group — stop
                        }
                    }
                }
                if let Some((row, total_segs)) = topmost {
                    let k = total_segs.saturating_sub(depth);
                    return Some((row, k));
                }
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
        let k = match self.cursor_section {
            CursorSection::Key { segment } => segment,
            CursorSection::Locale(_) => return None,
        };
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
