use std::collections::HashSet;
use tui_textarea::TextArea;
use crate::{
    editor::CellEdit,
    filter::{self, ColumnDirective},
    domain::DomainModel,
    store::{KeyId, NodeId},
    view_model::{self, ViewRow},
    workspace::Workspace,
};


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


// ── PasteState ────────────────────────────────────────────────────────────────

/// All clipboard and paste-panel state, grouped so that related fields and
/// their navigation methods live together.
#[derive(Debug, Default)]
pub struct PasteState {
    /// Per-locale yank history. Newest entry first; capped at 10 per locale.
    pub history:      std::collections::HashMap<String, Vec<String>>,
    /// Most recently yanked value, used for Ctrl+P quick-paste.
    pub last:         Option<String>,
    /// Focused locale column in the paste panel (index into the paste-locale list).
    pub locale_cursor: usize,
    /// Per-locale position of the `>` marker in the paste panel history list.
    pub history_pos:  std::collections::HashMap<String, usize>,
}

impl PasteState {
    /// Move the panel focus one locale to the left.
    pub fn nav_left(&mut self) {
        if self.locale_cursor > 0 {
            self.locale_cursor -= 1;
        }
    }

    /// Move the panel focus one locale to the right.
    pub fn nav_right(&mut self, n_locales: usize) {
        if self.locale_cursor + 1 < n_locales {
            self.locale_cursor += 1;
        }
    }

    /// Move the `>` selection marker up in `locale`'s history.
    pub fn nav_up(&mut self, locale: &str) {
        let pos = self.history_pos.entry(locale.to_string()).or_insert(0);
        if *pos > 0 { *pos -= 1; }
    }

    /// Move the `>` selection marker down in `locale`'s history.
    pub fn nav_down(&mut self, locale: &str) {
        let history_len = self.history.get(locale).map(|v| v.len()).unwrap_or(0);
        let pos = self.history_pos.entry(locale.to_string()).or_insert(0);
        if *pos + 1 < history_len { *pos += 1; }
    }

    /// Push `value` to the front of `locale`'s history (dedup, cap 10).
    /// Updates `last` and resets `history_pos` for this locale to 0.
    pub fn yank(&mut self, locale: String, value: String) {
        let history = self.history.entry(locale.clone()).or_insert_with(Vec::new);
        history.retain(|v| v != &value);
        history.insert(0, value.clone());
        history.truncate(10);
        self.history_pos.insert(locale, 0);
        self.last = Some(value);
    }

    /// Point `locale_cursor` at `locale` in `paste_locales`.
    /// When `locale` is `None`, only clamps the cursor to the valid range.
    /// Always clamps so an out-of-bounds cursor is corrected when locales shrink.
    pub fn focus_on_locale(&mut self, locale: Option<&str>, paste_locales: &[String]) {
        if let Some(loc) = locale {
            if let Some(idx) = paste_locales.iter().position(|l| l == loc) {
                self.locale_cursor = idx;
            }
        }
        self.locale_cursor = self.locale_cursor.min(paste_locales.len().saturating_sub(1));
    }

    /// Remove the `>` selected entry from `locale`'s history and clamp cursors.
    /// Returns `true` when the clipboard is now completely empty — the caller
    /// should close the paste panel.
    pub fn remove_entry(&mut self, locale: &str) -> bool {
        let pos = *self.history_pos.get(locale).unwrap_or(&0);
        if let Some(history) = self.history.get_mut(locale) {
            if pos < history.len() {
                history.remove(pos);
                if history.is_empty() {
                    self.history.remove(locale);
                    self.history_pos.remove(locale);
                    if self.history.is_empty() {
                        return true;
                    }
                    let remaining = self.history.len();
                    if self.locale_cursor >= remaining {
                        self.locale_cursor = remaining - 1;
                    }
                } else {
                    let new_len = history.len();
                    let p = self.history_pos.entry(locale.to_string()).or_insert(0);
                    if *p >= new_len { *p = new_len - 1; }
                }
            }
        }
        false
    }
}

#[derive(Debug)]
pub struct AppState {
    pub workspace: Workspace,
    /// Hierarchical domain model: single source of truth for rendered rows.
    pub domain_model: DomainModel,
    /// Locale columns currently visible; a subset of `workspace.all_locales()`
    /// when a locale selector is active, otherwise the full list.
    pub visible_locales: Vec<String>,
    pub scroll_offset: usize,
    pub mode: Mode,
    /// Always-present single-line TextArea backing the filter bar.
    /// The current query is `filter_textarea.lines()[0]`.
    pub filter_textarea: TextArea<'static>,
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
    /// Column visibility directive derived from `:?` / `:!` filter terms.
    /// Applied per-row in the table renderer to hide present/missing cells.
    pub column_directive: ColumnDirective,
    /// Clipboard and paste-panel state.
    pub paste: PasteState,
    /// Flat ordered list of every visual row, built from the domain model.
    /// Navigation indexes into this; the renderer paints a window of it.
    /// Rebuilt by `apply_filter` after every filter or workspace change.
    pub view_rows: Vec<ViewRow>,
    /// Index of the cursor's current row in `view_rows`.
    pub cursor_row: usize,
    /// Within-row segment offset from the rightmost segment (leaf = 0, toward root).
    /// Non-zero only for chain-collapsed rows where the user has pressed Left to
    /// walk the anchor toward the root without leaving the row.
    pub cursor_segment: usize,
    /// Selected locale column, or `None` when the cursor is in the key column.
    pub cursor_locale: Option<String>,
    /// Height of the properties table area from the last render (terminal rows).
    pub vp_height: usize,
    /// KeyIds that pass the current filter (including pinned/dirty bypass).
    /// Rebuilt by `apply_filter`; used by the `Children` scope in ops to
    /// determine which keys are currently visible.
    pub visible_key_ids: HashSet<KeyId>,
}



impl AppState {
    // ── Cursor helpers ────────────────────────────────────────────────────────

    /// The trie node for the current anchor, accounting for `cursor_segment`.
    /// `cursor_segment = 0` → the row's own node; `cursor_segment = n` → n levels up.
    /// Returns `None` only when `view_rows` is empty.
    pub fn anchor_node_id(&self) -> Option<NodeId> {
        let row = self.view_rows.get(self.cursor_row)?;
        if self.cursor_segment == 0 {
            return Some(row.identity.node_id);
        }
        let depth = self.domain_model.node_depth(row.identity.node_id)
            .saturating_sub(self.cursor_segment);
        Some(self.domain_model.node_ancestor_at_depth(row.identity.node_id, depth))
    }

    /// Bundle-qualified prefix string of the current anchor (workspace boundary).
    pub fn anchor_prefix(&self) -> String {
        self.anchor_node_id()
            .map(|nid| self.domain_model.node_qualified_str(nid))
            .unwrap_or_default()
    }

    /// Returns `true` when `bundle` has `locale` in the domain model.
    /// For bare (non-bundle) keys, always returns `true`.
    pub fn bundle_has_locale_in_model(&self, bundle: &str, locale: &str) -> bool {
        self.domain_model.bundle_has_locale(bundle, locale)
    }

    /// The 0-based index in `visible_locales` for the cursor's locale column.
    ///
    /// Returns `None` when the key column is active (`cursor_locale` is `None`).
    /// Snaps left to the nearest locale the current row's bundle owns if the
    /// preferred locale is not available for this bundle.
    pub fn effective_locale_idx(&self) -> Option<usize> {
        let locale = self.cursor_locale.as_ref()?;
        let bundle = self.view_rows.get(self.cursor_row)
            .map(|r| r.identity.bundle_name())
            .unwrap_or("");
        let pref_idx = self.visible_locales.iter().position(|l| l == locale)?;
        if bundle.is_empty() || self.bundle_has_locale_in_model(bundle, locale) {
            return Some(pref_idx);
        }
        // Snap left to nearest locale this bundle has.
        let mut i = pref_idx;
        loop {
            if self.bundle_has_locale_in_model(bundle, &self.visible_locales[i]) {
                return Some(i);
            }
            if i == 0 { break; }
            i -= 1;
        }
        None
    }

    /// Move to a locale column by index.
    pub fn set_locale_cursor(&mut self, idx: usize) {
        self.cursor_locale = self.visible_locales.get(idx).cloned();
    }

    /// Returns the current value at the cursor's effective locale cell.
    pub fn current_cell_value(&self) -> Option<String> {
        let locale_idx = self.effective_locale_idx()?;
        let locale = self.visible_locales.get(locale_idx)?;
        let row    = self.view_rows.get(self.cursor_row)?;
        let key_id = row.identity.key_id?;
        self.domain_model.translation_str(key_id, locale).map(|v| v.to_string())
    }

    /// Yank the value at the current cursor cell into `paste.history`.
    /// Returns `Some(locale)` on success, `None` when on the key column or
    /// the cell is empty.
    pub fn yank_cell(&mut self) -> Option<String> {
        let locale_idx = self.effective_locale_idx()?;
        let locale = self.visible_locales.get(locale_idx).cloned()?;
        let value  = self.current_cell_value()?;
        self.paste.yank(locale.clone(), value);
        Some(locale)
    }

    /// Returns the bundle name for the current cursor row, or `""` for bare keys.
    pub fn current_row_bundle(&self) -> &str {
        self.view_rows.get(self.cursor_row)
            .map(|r| r.identity.bundle_name())
            .unwrap_or("")
    }

    /// KeyIds that pass the current filter (including pinned/dirty bypass).
    /// Used by the `Children` scope in ops to determine which keys are visible.
    pub fn visible_key_ids(&self) -> HashSet<KeyId> {
        self.visible_key_ids.clone()
    }

    // ── Entry-based navigation ────────────────────────────────────────────────

    /// Bundle-qualified key string for display in the edit pane title.
    /// - Leaf: full key (`"messages:app.title"`)
    /// - Group header: prefix (`"messages:app"`)
    /// - Bundle header: `None`
    pub fn cursor_key_for_ops(&self) -> Option<String> {
        let id = &self.view_rows.get(self.cursor_row)?.identity;
        if id.is_leaf {
            id.key_id.map(|k| self.domain_model.key_qualified_str(k))
        } else if !id.is_bundle_header() {
            Some(id.prefix_str().to_string())
        } else {
            None
        }
    }

    /// NodeId for the current cursor row, for use as an op anchor.
    /// Returns `None` for bundle-header rows.
    pub fn cursor_node_id_for_ops(&self) -> Option<NodeId> {
        let id = &self.view_rows.get(self.cursor_row)?.identity;
        if id.is_bundle_header() { None } else { Some(id.node_id) }
    }

    /// KeyId for the current cursor row when it is a leaf key.
    /// Returns `None` for group/bundle header rows.
    pub fn cursor_key_id_for_ops(&self) -> Option<KeyId> {
        self.view_rows.get(self.cursor_row)?.identity.key_id
    }

    /// Move the cursor up one visual row (one KeyPartition).
    /// Move the cursor up one visual row.
    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
        }
        self.cursor_segment = 0;
        self.clamp_scroll();
    }

    /// Move the cursor down one visual row.
    pub fn move_down(&mut self) {
        if self.cursor_row + 1 < self.view_rows.len() {
            self.cursor_row += 1;
        }
        self.cursor_segment = 0;
        self.clamp_scroll();
    }

    /// Number of rows to move on a Page Up / Page Down action.
    pub fn page_size(&self) -> usize {
        self.vp_height.max(1)
    }

    /// Ensures `scroll_offset` keeps `cursor_row` inside the viewport.
    ///
    /// Invariant after this call:
    ///   `scroll_offset <= cursor_row < scroll_offset + vp_height`
    pub fn clamp_scroll(&mut self) {
        let vp = self.vp_height.max(1);
        if self.cursor_row < self.scroll_offset {
            self.scroll_offset = self.cursor_row;
        }
        if self.cursor_row + 1 > self.scroll_offset + vp {
            self.scroll_offset = self.cursor_row + 1 - vp;
        }
        let max_scroll = self.view_rows.len().saturating_sub(vp);
        self.scroll_offset = self.scroll_offset.min(max_scroll);
    }

    // ── Depth / child navigation helpers ─────────────────────────────────────

    /// Find the nearest row in direction `forward` at the same anchor depth
    /// (cousins qualify; reduces depth by 1 and retries if nothing is found).
    ///
    /// Returns the `view_rows` index of the target row, or `None` when no
    /// neighbor exists at any depth (falls back to plain up/down in update.rs).
    pub fn find_depth_neighbor(&self, forward: bool) -> Option<usize> {
        let anchor_nid = self.anchor_node_id()?;
        let target_d   = self.domain_model.node_depth(anchor_nid);
        if target_d == 0 { return None; }
        let bundle_id = self.domain_model.node_bundle_id(anchor_nid);

        for depth in (1..=target_d).rev() {
            let anchor_at_d = self.domain_model.node_ancestor_at_depth(anchor_nid, depth);

            if forward {
                // First row after cursor whose ancestor-at-depth differs.
                let hit = self.view_rows[self.cursor_row + 1..].iter().enumerate()
                    .find(|(_, r)| {
                        self.domain_model.node_bundle_id(r.identity.node_id) == bundle_id
                            && self.domain_model.node_ancestor_at_depth(r.identity.node_id, depth) != anchor_at_d
                    })
                    .map(|(i, _)| self.cursor_row + 1 + i);
                if hit.is_some() { return hit; }
            } else {
                // Topmost row of the nearest preceding sibling group.
                let mut sib_nid: Option<NodeId> = None;
                let mut first_row: Option<usize> = None;
                for i in (0..self.cursor_row).rev() {
                    let r = &self.view_rows[i];
                    if self.domain_model.node_bundle_id(r.identity.node_id) != bundle_id { break; }
                    let rp = self.domain_model.node_ancestor_at_depth(r.identity.node_id, depth);
                    if rp == anchor_at_d {
                        // Entered our own group going backward — stop if target found.
                        if sib_nid.is_some() { break; }
                        continue;
                    }
                    match sib_nid {
                        None => { sib_nid = Some(rp); first_row = Some(i); }
                        Some(s) if s == rp => { first_row = Some(i); }
                        _ => break,
                    }
                }
                if first_row.is_some() { return first_row; }
            }
        }
        None
    }

    /// Find the first child row of the current anchor in `view_rows`.
    ///
    /// - Bundle header: first non-header row in the same bundle.
    /// - Any other row: first row after cursor whose prefix starts with
    ///   `anchor_prefix + "."`.
    ///
    /// Returns the `view_rows` index, or `None`.
    pub fn find_first_child_row(&self) -> Option<usize> {
        let row = self.view_rows.get(self.cursor_row)?;
        let id  = &row.identity;

        if id.is_bundle_header() {
            // Bundle header: first content row in the same bundle.
            let bundle = id.bundle_name().to_string();
            return self.view_rows[self.cursor_row + 1..].iter().enumerate()
                .find(|(_, r)| r.identity.bundle_name() == bundle)
                .map(|(i, _)| self.cursor_row + 1 + i);
        }

        let anchor_nid = self.anchor_node_id()?;
        self.view_rows[self.cursor_row + 1..].iter().enumerate()
            .find(|(_, r)| self.domain_model.node_is_strict_ancestor(anchor_nid, r.identity.node_id))
            .map(|(i, _)| self.cursor_row + 1 + i)
    }

    // ── Composite navigation moves ────────────────────────────────────────────

    /// Left arrow: step left through this bundle's locale columns, falling back
    /// to the key column when no locale to the left is owned by the bundle.
    /// In the key column: walk the anchor one level toward the bundle root
    /// (jump to the parent row, or increment `cursor_segment` when the parent
    /// is chain-collapsed into the current row).
    /// Returns `true` when a scroll/temp-pin refresh is needed.
    pub fn move_cursor_left(&mut self) -> bool {
        if self.cursor_locale.is_some() {
            let bundle = self.current_row_bundle().to_string();
            let cur = self.visible_locales.iter()
                .position(|l| Some(l) == self.cursor_locale.as_ref())
                .unwrap_or(0);
            let target = (0..cur).rev()
                .find(|&i| bundle.is_empty() || self.bundle_has_locale_in_model(&bundle, &self.visible_locales[i]));
            match target {
                Some(i) => { self.set_locale_cursor(i); }
                None    => { self.cursor_locale = None; }
            }
            false
        } else {
            // Key column: walk anchor one level toward the bundle root.
            let anchor_nid = match self.anchor_node_id() {
                Some(nid) => nid,
                None => return false,
            };
            match self.domain_model.node_parent_id(anchor_nid) {
                Some(parent_nid) => {
                    let target = self.view_rows[..self.cursor_row].iter()
                        .enumerate().rev()
                        .find(|(_, r)| r.identity.node_id == parent_nid)
                        .map(|(i, _)| i);
                    if let Some(row_idx) = target {
                        self.cursor_row     = row_idx;
                        self.cursor_segment = 0;
                    } else {
                        // Chain-collapsed interior: shift highlight left within the row.
                        self.cursor_segment += 1;
                    }
                    self.clamp_scroll();
                    true
                }
                None => false, // already at bundle root — no-op
            }
        }
    }

    /// Right arrow: step right through this bundle's locale columns (no-op at
    /// the rightmost owned locale).  In the key column: walk the anchor back
    /// toward the leaf (decrement `cursor_segment`), or jump to the first
    /// owned locale when already at the leaf.
    /// Returns `true` when a temp-pin refresh is needed.
    pub fn move_cursor_right(&mut self) -> bool {
        if self.cursor_locale.is_none() {
            // Key column.
            if self.cursor_segment > 0 {
                self.cursor_segment -= 1;
                false
            } else {
                // At leaf: jump to the first locale this bundle owns.
                let bundle = self.current_row_bundle().to_string();
                let target = (0..self.visible_locales.len())
                    .find(|&i| bundle.is_empty() || self.bundle_has_locale_in_model(&bundle, &self.visible_locales[i]));
                if let Some(i) = target { self.set_locale_cursor(i); }
                false
            }
        } else {
            // Locale column: step right through owned locales.
            let bundle = self.current_row_bundle().to_string();
            let cur = self.visible_locales.iter()
                .position(|l| Some(l) == self.cursor_locale.as_ref())
                .unwrap_or(0);
            let target = (cur + 1..self.visible_locales.len())
                .find(|&i| bundle.is_empty() || self.bundle_has_locale_in_model(&bundle, &self.visible_locales[i]));
            if let Some(i) = target { self.set_locale_cursor(i); }
            false // at rightmost locale → no-op; no temp-pin change needed
        }
    }

    /// Ctrl+→: jump to the first child of the current anchor and reset the segment.
    /// Returns `true` when a scroll/temp-pin refresh is needed.
    pub fn move_to_first_child(&mut self) -> bool {
        if let Some(row_idx) = self.find_first_child_row() {
            self.cursor_row     = row_idx;
            self.cursor_segment = 0;
            self.cursor_locale  = None;
            self.clamp_scroll();
            true
        } else {
            false
        }
    }

    /// Shift+↑: jump to the nearest bundle-level header row above the cursor,
    /// skipping the bundle the cursor is currently in.
    pub fn jump_to_prev_bundle(&mut self) {
        let bundle = self.current_row_bundle().to_string();
        let target = self.view_rows[..self.cursor_row].iter().enumerate().rev()
            .find(|(_, r)| r.identity.is_bundle_header() && r.identity.bundle_name() != bundle)
            .map(|(i, _)| i);
        if let Some(row_idx) = target {
            self.cursor_row     = row_idx;
            self.cursor_segment = 0;
            self.cursor_locale  = None;
            self.clamp_scroll();
        }
    }

    /// Shift+↓: jump to the nearest bundle-level header row below the cursor,
    /// skipping the bundle the cursor is currently in.
    pub fn jump_to_next_bundle(&mut self) {
        let bundle = self.current_row_bundle().to_string();
        let target = self.view_rows[self.cursor_row + 1..].iter().enumerate()
            .find(|(_, r)| r.identity.is_bundle_header() && r.identity.bundle_name() != bundle)
            .map(|(i, _)| self.cursor_row + 1 + i);
        if let Some(row_idx) = target {
            self.cursor_row     = row_idx;
            self.cursor_segment = 0;
            self.cursor_locale  = None;
            self.clamp_scroll();
        }
    }

    /// PgUp: move up by one viewport height.
    pub fn page_up(&mut self) {
        let n = self.page_size();
        for _ in 0..n { self.move_up(); }
    }

    /// PgDn: move down by one viewport height.
    pub fn page_down(&mut self) {
        let n = self.page_size();
        for _ in 0..n { self.move_down(); }
    }

    // ── Filter ────────────────────────────────────────────────────────────────

    /// Re-evaluates the filter query, rebuilds `view_rows` and `visible_locales`,
    /// then clamps the cursor.
    pub fn apply_filter(&mut self) {
        let query = self.filter_textarea.lines()[0].clone();
        // Save identity of the current row so we can restore cursor position after rebuild.
        let saved = self.view_rows.get(self.cursor_row)
            .map(|r| (r.identity.node_id, r.identity.is_leaf, r.identity.bundle_name().to_string()));

        let (visible, directive, descriptors) = if query.trim().is_empty() {
            let visible = self.domain_model.all_locale_strings();
            let descs   = self.domain_model.visible_rows(|_| true);
            (visible, ColumnDirective::None, descs)
        } else {
            let expr        = filter::parse(&query);
            let dirty_locs  = self.domain_model.dirty_locale_strings();
            let visible     = filter::visible_locales(&expr, &self.domain_model, &dirty_locs);
            let directive   = filter::column_directive(&expr);
            let dm          = &self.domain_model;
            let descs = dm.visible_rows(|kid| {
                filter::evaluate(&expr, kid, dm)
                    || dm.is_temp_pinned(kid) || dm.is_pinned(kid) || dm.is_dirty(kid)
            });
            (visible, directive, descs)
        };

        self.visible_locales  = visible;
        self.column_directive = directive;
        self.view_rows = view_model::enrich_rows(
            descriptors,
            &self.domain_model,
            &self.visible_locales,
            self.column_directive,
        );
        self.visible_key_ids = self.view_rows.iter()
            .filter_map(|r| r.identity.key_id)
            .collect();
        self.cursor_row = if let Some((node_id, is_leaf, bundle)) = saved {
            self.view_rows.iter().position(|r| {
                r.identity.node_id == node_id && r.identity.is_leaf == is_leaf
            })
            .or_else(|| self.view_rows.iter().position(|r| r.identity.bundle_name() == bundle))
            .unwrap_or(0)
        } else {
            0
        }.min(self.view_rows.len().saturating_sub(1));
        if let Some(ref loc) = self.cursor_locale.clone() {
            if !self.visible_locales.contains(loc) {
                self.cursor_locale = None;
            }
        }
        self.clamp_scroll();
    }

    // ── Clipboard ─────────────────────────────────────────────────────────────

    /// Returns clipboard locales in table column order (`visible_locales` first,
    /// then any clipboard locales not currently visible, alphabetically).
    pub fn paste_locales(&self) -> Vec<String> {
        let visible_set: std::collections::HashSet<&str> =
            self.visible_locales.iter().map(|s| s.as_str()).collect();
        let mut keys: Vec<String> = self.visible_locales.iter()
            .filter(|l| self.paste.history.contains_key(*l))
            .cloned()
            .collect();
        let mut extra: Vec<String> = self.paste.history.keys()
            .filter(|l| !visible_set.contains(l.as_str()))
            .cloned()
            .collect();
        extra.sort();
        keys.extend(extra);
        keys
    }

    // ── Constructor ───────────────────────────────────────────────────────────

    pub fn new(workspace: Workspace) -> Self {
        let domain_model    = DomainModel::from_workspace(&workspace);
        let visible_locales = domain_model.all_locale_strings();
        let descriptors     = domain_model.visible_rows(|_| true);
        let view_rows = view_model::enrich_rows(
            descriptors,
            &domain_model,
            &visible_locales,
            ColumnDirective::None,
        );
        let visible_key_ids: HashSet<KeyId> = view_rows.iter()
            .filter_map(|r| r.identity.key_id)
            .collect();
        Self {
            workspace,
            domain_model,
            visible_locales,
            visible_key_ids,
            scroll_offset: 0,
            mode: Mode::Normal,
            filter_textarea: TextArea::default(),
            quitting: false,
            edit_buffer: None,
            selection_scope: SelectionScope::Exact,
            status_message: None,
            show_preview: false,
            column_directive: ColumnDirective::None,
            paste: PasteState::default(),
            view_rows,
            cursor_row: 0,
            cursor_segment: 0,
            cursor_locale: None,
            vp_height: 0,
        }
    }
}
