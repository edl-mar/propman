use std::collections::HashSet;
use std::path::PathBuf;
use tui_textarea::TextArea;
use crate::{
    editor::CellEdit,
    filter::{self, ColumnDirective},
    render_model::{self, RenderModel, VisualPosition},
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

/// The structured cursor.  Replaces the old `cursor_row + CursorSection + preferred_locale`.
///
/// `bundle`   — bundle name; `None` for bare/legacy keys.
/// `segments` — key path within the bundle.  Empty = bundle header row.
///              Non-empty = group header row or specific entry row.
/// `locale`   — locale column; `None` = key column.
///              Stored as the "preferred" locale — may not exist in the current
///              bundle; `effective_locale_idx()` resolves the actual visible column.
#[derive(Debug, Clone, PartialEq)]
pub struct Cursor {
    pub bundle:   Option<String>,
    pub segments: Vec<String>,
    pub locale:   Option<String>,
}

impl Cursor {
    /// True when the cursor is in the key column.
    pub fn is_key_col(&self) -> bool { self.locale.is_none() }

    /// True when the cursor is on a bundle header row.
    pub fn is_bundle_header(&self) -> bool { self.segments.is_empty() }

    /// Bundle-qualified full key string, or `None` when on a bundle header.
    pub fn full_key(&self) -> Option<String> {
        if self.is_bundle_header() { return None; }
        let key = self.segments.join(".");
        Some(match &self.bundle {
            Some(b) => format!("{b}:{key}"),
            None    => key,
        })
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
    /// Hierarchical render model: single source of truth for rendered rows.
    pub render_model: RenderModel,
    /// Locale columns currently visible; a subset of `workspace.all_locales()`
    /// when a locale selector is active, otherwise the full list.
    pub visible_locales: Vec<String>,
    /// Structured cursor: replaces the old `cursor_row + CursorSection + preferred_locale`.
    pub cursor: Cursor,
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
    // ── Cursor helpers ────────────────────────────────────────────────────────

    /// The 0-based index in `visible_locales` that the cursor is effectively on.
    ///
    /// `cursor.locale` stores the *preferred* locale and may not exist in the
    /// current bundle.  This method snaps left to the nearest available column,
    /// returning `None` when no locale column is reachable (key column active).
    pub fn effective_locale_idx(&self) -> Option<usize> {
        let locale = self.cursor.locale.as_ref()?;
        let bundle = self.cursor.bundle.as_deref().unwrap_or("");
        let pref_idx = self.visible_locales.iter().position(|l| l == locale)?;
        // Preferred locale is valid for this bundle → use it directly.
        if bundle.is_empty() || self.workspace.bundle_has_locale(bundle, locale) {
            return Some(pref_idx);
        }
        // Snap left to the nearest locale this bundle has.
        let mut i = pref_idx;
        loop {
            if bundle.is_empty() || self.workspace.bundle_has_locale(bundle, &self.visible_locales[i]) {
                return Some(i);
            }
            if i == 0 { break; }
            i -= 1;
        }
        None
    }

    /// Move to a locale column by index. Updates `cursor.locale` to the locale name.
    /// Always call this for explicit user-initiated locale navigation.
    pub fn set_locale_cursor(&mut self, idx: usize) {
        self.cursor.locale = self.visible_locales.get(idx).cloned();
    }

    /// Returns the current value at the cursor's effective locale cell.
    pub fn current_cell_value(&self) -> Option<String> {
        let locale_idx = self.effective_locale_idx()?;
        let locale = self.visible_locales.get(locale_idx)?;
        let full_key = self.cursor.full_key()?;
        self.workspace.get_value(&full_key, locale).map(|v| v.to_string())
    }

    /// Returns the bundle name for the current cursor row, or `""` for bare keys.
    pub fn current_row_bundle(&self) -> &str {
        self.cursor.bundle.as_deref().unwrap_or("")
    }

    /// Bundle-qualified full-key strings for every entry currently in the render model.
    /// Used by ops to determine which keys are filter-visible (the `Children` scope).
    pub fn visible_full_keys(&self) -> HashSet<String> {
        self.render_model.bundles.iter()
            .flat_map(|b| b.entries.iter().map(move |e| {
                let key = e.segments.join(".");
                if b.name.is_empty() { key } else { format!("{}:{}", b.name, key) }
            }))
            .collect()
    }

    // ── Visual-row navigation ─────────────────────────────────────────────────

    /// Enumerate all visual row positions in the render model.
    /// Cached as a local Vec; called at most a few times per keypress.
    pub fn visual_positions(&self) -> Vec<VisualPosition> {
        render_model::all_visual_positions(&self.render_model)
    }

    /// Flat visual row index for the current cursor position.
    ///
    /// When `cursor.segments` exactly matches a visual row, returns that row.
    /// When it is a prefix anchor (chain-collapsed intermediate node with no
    /// visual row of its own), returns the first visual row whose segments
    /// start with `cursor.segments` — that is the "effective entry" shown for
    /// the anchor.
    pub fn cursor_visual_row(&self) -> usize {
        let positions = self.visual_positions();
        let segs = &self.cursor.segments;
        // Exact match first.
        if let Some(idx) = positions.iter().position(|p|
            p.bundle == self.cursor.bundle && p.segments == *segs
        ) {
            return idx;
        }
        // Prefix-anchor fallback: first descendant row.
        if !segs.is_empty() {
            if let Some(idx) = positions.iter().position(|p|
                p.bundle == self.cursor.bundle && p.segments.starts_with(segs.as_slice())
            ) {
                return idx;
            }
        }
        0
    }

    /// Total number of visual rows.
    pub fn total_visual_rows(&self) -> usize {
        self.visual_positions().len()
    }

    /// Navigate to visual row `row`, preserving `cursor.locale`.
    /// Does nothing when `row` is out of range.
    pub fn set_visual_row(&mut self, row: usize) {
        let positions = self.visual_positions();
        if let Some(pos) = positions.get(row) {
            self.cursor.bundle   = pos.bundle.clone();
            self.cursor.segments = pos.segments.clone();
        }
    }

    /// Clamps the cursor to a valid position in the current render model.
    ///
    /// Called after `apply_filter` rebuilds the model.  A cursor position is
    /// valid when:
    ///   - it exactly matches a visual row (exact entry or group header), OR
    ///   - `cursor.segments` is non-empty and is a prefix of some visual row
    ///     (key-segment anchor pointing into a chain-collapsed node that has no
    ///     visual row of its own, but its descendant entry is still visible).
    ///
    /// If neither holds, walk up the segment stack until a valid position is
    /// found, then fall back to row 0.
    fn clamp_cursor_to_model(&mut self) {
        let positions = self.visual_positions();
        if positions.is_empty() { return; }

        let is_valid = |segs: &Vec<String>| {
            positions.iter().any(|p| {
                p.bundle == self.cursor.bundle
                    && (p.segments == *segs
                        || (!segs.is_empty() && p.segments.starts_with(segs.as_slice())))
            })
        };

        if is_valid(&self.cursor.segments) { return; }

        // Walk up until we find a valid anchor (exact row or valid prefix).
        let mut segs = self.cursor.segments.clone();
        while !segs.is_empty() {
            segs.pop();
            if is_valid(&segs) {
                self.cursor.segments = segs;
                return;
            }
        }
        // Fall back to row 0 (first visible row).
        let first = &positions[0];
        self.cursor.bundle   = first.bundle.clone();
        self.cursor.segments = first.segments.clone();
    }

    /// Keeps `scroll_offset` in sync so the cursor stays visible.
    pub fn clamp_scroll(&mut self) {
        const VIEWPORT: usize = 20;
        let row = self.cursor_visual_row();
        if row < self.scroll_offset {
            self.scroll_offset = row;
        } else if row >= self.scroll_offset + VIEWPORT {
            self.scroll_offset = row - VIEWPORT + 1;
        }
    }

    // ── Key-segment navigation helpers ────────────────────────────────────────

    /// Find the nearest visual position in direction `forward` at the same segment
    /// depth as the cursor, within the same bundle.
    ///
    /// If no row exists at the current depth in that direction, reduces depth by
    /// 1 and retries.  For backward direction returns the topmost row of the
    /// nearest previous group at that depth.
    ///
    /// Returns `(bundle, segments)` of the target row, or `None`.
    pub fn find_depth_neighbor(&self, forward: bool) -> Option<(Option<String>, Vec<String>)> {
        let target_depth = self.cursor.segments.len();
        if target_depth == 0 { return None; }

        let positions = self.visual_positions();
        let current_row = positions.iter()
            .position(|p| p.bundle == self.cursor.bundle && p.segments == self.cursor.segments)
            .unwrap_or(0);

        for depth in (1..=target_depth).rev() {
            // The anchor prefix at this depth (first `depth` segments of cursor).
            let anchor_at_depth: &[String] = &self.cursor.segments[..depth];

            if forward {
                for pos in positions[current_row + 1..].iter() {
                    if pos.bundle != self.cursor.bundle { continue; }
                    if pos.segments.len() < depth { continue; }
                    if pos.segments[..depth] != *anchor_at_depth {
                        return Some((pos.bundle.clone(), pos.segments.clone()));
                    }
                }
            } else {
                let mut found_prefix: Option<Vec<String>> = None;
                let mut topmost_idx: Option<usize> = None;
                for i in (0..current_row).rev() {
                    let pos = &positions[i];
                    if pos.bundle != self.cursor.bundle { continue; }
                    if pos.segments.len() < depth { continue; }
                    let pos_pfx = pos.segments[..depth].to_vec();
                    match &found_prefix {
                        None => {
                            if pos_pfx != anchor_at_depth {
                                found_prefix = Some(pos_pfx);
                                topmost_idx  = Some(i);
                            }
                        }
                        Some(fp) if *fp == pos_pfx => {
                            topmost_idx = Some(i); // keep climbing
                        }
                        _ => break,
                    }
                }
                if let Some(idx) = topmost_idx {
                    let pos = &positions[idx];
                    return Some((pos.bundle.clone(), pos.segments.clone()));
                }
            }
        }
        None
    }

    /// Returns true when `cursor.segments` is a prefix anchor — i.e. the cursor
    /// points to an intermediate node that has no visual row of its own but is
    /// "inside" a chain-collapsed entry that does have a visual row.
    ///
    /// Used by Left/Right to decide whether to pop/push a segment or to
    /// switch to a locale column.
    pub fn cursor_is_prefix_anchor(&self) -> bool {
        if self.cursor.segments.is_empty() { return false; }
        let positions = self.visual_positions();
        !positions.iter().any(|p| p.bundle == self.cursor.bundle && p.segments == self.cursor.segments)
            && positions.iter().any(|p| p.bundle == self.cursor.bundle && p.segments.starts_with(self.cursor.segments.as_slice()))
    }

    /// Find the first visual position that is a (direct or indirect) child of the cursor.
    /// Used by Ctrl+Right (GoToFirstChild).
    pub fn find_first_child_position(&self) -> Option<(Option<String>, Vec<String>)> {
        let positions = self.visual_positions();
        let current_row = positions.iter()
            .position(|p| p.bundle == self.cursor.bundle && p.segments == self.cursor.segments)
            .unwrap_or(0);
        let parent_segs = &self.cursor.segments;
        positions[current_row + 1..].iter().find(|p| {
            p.bundle == self.cursor.bundle
                && p.segments.len() > parent_segs.len()
                && p.segments[..parent_segs.len()] == *parent_segs
        }).map(|p| (p.bundle.clone(), p.segments.clone()))
    }

    // ── Filter ────────────────────────────────────────────────────────────────

    /// Derives the set of locales that have at least one pending (unsaved) write.
    fn compute_dirty_locales(&self) -> HashSet<String> {
        let path_to_locale: std::collections::HashMap<&std::path::Path, &str> = self
            .workspace.groups.iter()
            .flat_map(|g| g.files.iter())
            .map(|f| (f.path.as_path(), f.locale.as_str()))
            .collect();
        self.pending_writes.iter()
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

    /// Derives `(full_key, locale)` pairs that have at least one pending write.
    fn compute_dirty_cells(&self) -> HashSet<(String, String)> {
        let path_to_locale: std::collections::HashMap<&std::path::Path, &str> = self
            .workspace.groups.iter()
            .flat_map(|g| g.files.iter())
            .map(|f| (f.path.as_path(), f.locale.as_str()))
            .collect();
        self.pending_writes.iter()
            .filter_map(|c| {
                let (path, full_key) = match c {
                    PendingChange::Update { path, full_key, .. } => (path.as_path(), full_key.as_str()),
                    PendingChange::Insert { path, full_key, .. } => (path.as_path(), full_key.as_str()),
                    PendingChange::Delete { path, full_key, .. } => (path.as_path(), full_key.as_str()),
                };
                path_to_locale.get(path)
                    .map(|locale| (full_key.to_string(), locale.to_string()))
            })
            .collect()
    }

    /// Re-evaluates the filter query, rebuilds `render_model` and `visible_locales`,
    /// then clamps the cursor.
    pub fn apply_filter(&mut self) {
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
        let dirty_cells  = self.compute_dirty_cells();
        self.render_model = render_model::build_render_model(
            &self.workspace,
            &filtered,
            &bundle_names,
            &visible,
            &self.dirty_keys,
            &dirty_cells,
            &self.pinned_keys,
            &self.temp_pins,
        );
        self.visible_locales = visible;
        self.column_directive = directive;
        // Cursor is its own identity — just clamp it to the new model.
        self.clamp_cursor_to_model();
        self.clamp_scroll();
    }

    // ── Clipboard ─────────────────────────────────────────────────────────────

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

    // ── Constructor ───────────────────────────────────────────────────────────

    pub fn new(workspace: Workspace) -> Self {
        let bundle_names  = workspace.bundle_names();
        let visible_locales = workspace.all_locales();
        let empty_keys: HashSet<String> = HashSet::new();
        let empty_cells: HashSet<(String, String)> = HashSet::new();
        let rm = render_model::build_render_model(
            &workspace,
            &workspace.merged_keys,
            &bundle_names,
            &visible_locales,
            &empty_keys,
            &empty_cells,
            &empty_keys,
            &[],
        );
        // Start at row 0.
        let first_pos = render_model::all_visual_positions(&rm).into_iter().next();
        let cursor = if let Some(pos) = first_pos {
            Cursor { bundle: pos.bundle, segments: pos.segments, locale: None }
        } else {
            Cursor { bundle: None, segments: vec![], locale: None }
        };
        Self {
            workspace,
            render_model: rm,
            visible_locales,
            cursor,
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
