use std::collections::HashSet;
use std::path::PathBuf;
use tui_textarea::TextArea;
use crate::{
    editor::CellEdit,
    filter::{self, ColumnDirective},
    app_model::{self, DomainModel},
    view_model::{self, ViewRow},
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

// ── Prefix helpers (free functions used by navigation) ────────────────────────

/// Number of key segments in the bundle-qualified prefix (bundle prefix excluded).
/// "messages:app.confirm" → 2, "messages" → 0, "app.confirm" → 2, "" → 0.
fn prefix_depth(prefix: &str) -> usize {
    let key_part = match prefix.find(':') {
        Some(i) => &prefix[i + 1..],
        None    => prefix,
    };
    if key_part.is_empty() { 0 } else { key_part.split('.').count() }
}

/// Trim the bundle-qualified prefix to `depth` key segments.
/// prefix_at_depth("messages:app.confirm.delete", 2) → "messages:app.confirm"
/// prefix_at_depth("messages:app", 0) → "messages"
fn prefix_at_depth(prefix: &str, depth: usize) -> String {
    let (bundle_colon, key_part) = match prefix.find(':') {
        Some(i) => (&prefix[..=i], &prefix[i + 1..]),
        None    => ("", prefix),
    };
    let segs: Vec<&str> = key_part.split('.').collect();
    let keep = segs.len().min(depth);
    if keep == 0 {
        bundle_colon.trim_end_matches(':').to_string()
    } else {
        format!("{}{}", bundle_colon, segs[..keep].join("."))
    }
}

/// Immediate parent of a bundle-qualified prefix.
/// "messages:app.confirm" → Some("messages:app")
/// "messages:app"         → Some("messages")  (bundle header)
/// "messages"             → None
/// "app.confirm"          → Some("app")
/// "app"                  → None
fn parent_prefix(prefix: &str) -> Option<String> {
    if let Some(colon) = prefix.find(':') {
        let key_part = &prefix[colon + 1..];
        if key_part.contains('.') {
            let last_dot = key_part.rfind('.')?;
            Some(format!("{}:{}", &prefix[..colon], &key_part[..last_dot]))
        } else {
            // Only one key segment — parent is the bundle header (just the bundle name).
            Some(prefix[..colon].to_string())
        }
    } else {
        // Bare key: parent is the prefix up to the last dot.
        let last_dot = prefix.rfind('.')?;
        Some(prefix[..last_dot].to_string())
    }
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
}



impl AppState {
    // ── Cursor helpers ────────────────────────────────────────────────────────

    /// Bundle-qualified prefix of the current anchor, accounting for
    /// `cursor_segment` within chain-collapsed rows.
    /// `cursor_segment = 0` → full row prefix; `cursor_segment = n` → n segments
    /// stripped from the right.
    pub fn anchor_prefix(&self) -> String {
        let Some(row) = self.view_rows.get(self.cursor_row) else { return String::new() };
        if self.cursor_segment == 0 {
            return row.identity.prefix.clone();
        }
        prefix_at_depth(
            &row.identity.prefix,
            prefix_depth(&row.identity.prefix).saturating_sub(self.cursor_segment),
        )
    }

    /// Immediate parent of the current anchor prefix, or `None` when the
    /// anchor is already at the bundle-header level (no parent exists).
    pub fn anchor_parent_prefix(&self) -> Option<String> {
        parent_prefix(&self.anchor_prefix())
    }

    /// Returns `true` when `bundle` has `locale` in the current render model.
    /// For bare (non-bundle) keys, always returns `true`.
    /// This replaces `workspace.bundle_has_locale()` calls above the build boundary.
    pub fn bundle_has_locale_in_model(&self, bundle: &str, locale: &str) -> bool {
        if bundle.is_empty() { return true; }
        self.domain_model.bundles.iter()
            .find(|b| b.name == bundle)
            .map_or(false, |bm| bm.locales.iter().any(|l| l == locale))
    }

    /// The 0-based index in `visible_locales` for the cursor's locale column.
    ///
    /// Returns `None` when the key column is active (`cursor_locale` is `None`).
    /// Snaps left to the nearest locale the current row's bundle owns if the
    /// preferred locale is not available for this bundle.
    pub fn effective_locale_idx(&self) -> Option<usize> {
        let locale = self.cursor_locale.as_ref()?;
        let bundle = self.view_rows.get(self.cursor_row)
            .map(|r| r.identity.bundle.as_str())
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
    /// Reads from `Entry.cells` in the render model — no workspace access.
    pub fn current_cell_value(&self) -> Option<String> {
        let locale_idx = self.effective_locale_idx()?;
        let locale = self.visible_locales.get(locale_idx)?;
        let row = self.view_rows.get(self.cursor_row)?;
        let full_key = row.identity.full_key.as_deref()?;
        let (bundle_name, real_key) = workspace::split_key(full_key);
        let bundle = self.domain_model.bundles.iter().find(|b| b.name == bundle_name)?;
        let cell_idx = bundle.locales.iter().position(|l| l == locale)?;
        let entry = bundle.entries.iter().find(|e| e.real_key() == real_key)?;
        entry.cells.get(cell_idx)?.value.clone()
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
            .map(|r| r.identity.bundle.as_str())
            .unwrap_or("")
    }

    /// Bundle-qualified full-key strings for every entry currently in the render model.
    /// Used by ops to determine which keys are filter-visible (the `Children` scope).
    pub fn visible_full_keys(&self) -> HashSet<String> {
        self.domain_model.bundles.iter()
            .flat_map(|b| b.entries.iter().map(move |e| {
                let key = e.segments.join(".");
                if b.name.is_empty() { key } else { format!("{}:{}", b.name, key) }
            }))
            .collect()
    }

    // ── Entry-based navigation ────────────────────────────────────────────────

    /// Bundle-qualified key for rename/delete/pin operations.
    ///
    /// - Leaf: full key (`"messages:app.title"`)
    /// - Group header: prefix (`"messages:app"`) — the operation target
    /// - Bundle header: `None`
    pub fn cursor_key_for_ops(&self) -> Option<String> {
        let id = &self.view_rows.get(self.cursor_row)?.identity;
        if id.is_leaf {
            id.full_key.clone()
        } else if id.prefix != id.bundle {
            Some(id.prefix.clone())
        } else {
            None
        }
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
        let anchor    = self.anchor_prefix();
        let target_d  = prefix_depth(&anchor);
        if target_d == 0 { return None; }
        let bundle = self.view_rows.get(self.cursor_row)?.identity.bundle.as_str();

        for depth in (1..=target_d).rev() {
            let anchor_at_d = prefix_at_depth(&anchor, depth);

            if forward {
                // First row after cursor whose prefix-at-depth differs.
                let hit = self.view_rows[self.cursor_row + 1..].iter().enumerate()
                    .find(|(_, r)| {
                        r.identity.bundle == bundle
                            && prefix_at_depth(&r.identity.prefix, depth) != anchor_at_d
                    })
                    .map(|(i, _)| self.cursor_row + 1 + i);
                if hit.is_some() { return hit; }
            } else {
                // Topmost row of the nearest preceding sibling group.
                let mut sib_pfx: Option<String> = None;
                let mut first_row: Option<usize> = None;
                for i in (0..self.cursor_row).rev() {
                    let r = &self.view_rows[i];
                    if r.identity.bundle != bundle { break; }
                    let rp = prefix_at_depth(&r.identity.prefix, depth);
                    if rp == anchor_at_d {
                        // Entered our own group going backward — stop if target found.
                        if sib_pfx.is_some() { break; }
                        continue;
                    }
                    match &sib_pfx {
                        None => { sib_pfx = Some(rp); first_row = Some(i); }
                        Some(sp) if *sp == rp => { first_row = Some(i); }
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

        if !id.is_leaf && id.prefix == id.bundle {
            // Bundle header: first content row in the same bundle.
            return self.view_rows[self.cursor_row + 1..].iter().enumerate()
                .find(|(_, r)| r.identity.bundle == id.bundle)
                .map(|(i, _)| self.cursor_row + 1 + i);
        }

        let child_pfx = format!("{}.", self.anchor_prefix());
        self.view_rows[self.cursor_row + 1..].iter().enumerate()
            .find(|(_, r)| r.identity.prefix.starts_with(&child_pfx))
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
            if let Some(parent) = self.anchor_parent_prefix() {
                let cur_bundle = self.current_row_bundle().to_string();
                let target = self.view_rows[..self.cursor_row].iter()
                    .enumerate().rev()
                    .find(|(_, r)| r.identity.bundle == cur_bundle && r.identity.prefix == parent)
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
            } else {
                false // already at bundle root — no-op
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
            .find(|(_, r)| {
                !r.identity.is_leaf
                    && r.identity.prefix == r.identity.bundle
                    && !r.identity.bundle.is_empty()
                    && r.identity.bundle != bundle
            })
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
            .find(|(_, r)| {
                !r.identity.is_leaf
                    && r.identity.prefix == r.identity.bundle
                    && !r.identity.bundle.is_empty()
                    && r.identity.bundle != bundle
            })
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

    /// Re-evaluates the filter query, rebuilds `app_model` and `visible_locales`,
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
        self.domain_model = app_model::build_domain_model(
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
        // Save identity of the current row so we can restore cursor position after rebuild.
        let saved = self.view_rows.get(self.cursor_row)
            .map(|r| (r.identity.prefix.clone(), r.identity.is_leaf, r.identity.bundle.clone()));

        self.view_rows = view_model::build_view_rows(
            &self.domain_model,
            &self.visible_locales,
            self.column_directive,
        );
        self.cursor_row = if let Some((prefix, is_leaf, bundle)) = saved {
            // Exact match first — restores group-header rows correctly.
            self.view_rows.iter().position(|r| {
                r.identity.prefix == prefix && r.identity.is_leaf == is_leaf
            })
            // Row was filtered out — land on any row in the same bundle.
            .or_else(|| self.view_rows.iter().position(|r| r.identity.bundle == bundle))
            .unwrap_or(0)
        } else {
            0
        }.min(self.view_rows.len().saturating_sub(1));
        // Drop cursor_locale if it is no longer in visible_locales.
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
        let bundle_names  = workspace.bundle_names();
        let visible_locales = workspace.all_locales();
        let empty_keys: HashSet<String> = HashSet::new();
        let empty_cells: HashSet<(String, String)> = HashSet::new();
        let rm = app_model::build_domain_model(
            &workspace,
            &workspace.merged_keys,
            &bundle_names,
            &visible_locales,
            &empty_keys,
            &empty_cells,
            &empty_keys,
            &[],
        );
        let view_rows = view_model::build_view_rows(&rm, &visible_locales, ColumnDirective::None);
        Self {
            workspace,
            domain_model: rm,
            visible_locales,
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
            paste: PasteState::default(),
            view_rows,
            cursor_row: 0,
            cursor_segment: 0,
            cursor_locale: None,
            vp_height: 0,
        }
    }
}
