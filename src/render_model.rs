use std::collections::{BTreeMap, HashMap, HashSet};
use crate::workspace;
// BTreeMap is used only in build_render_model (to keep bundles in alphabetical order).

#[derive(Debug, Clone)]
pub enum DisplayRow {
    /// A non-key branch point that acts as a visual group header.
    /// `display` is the text shown (relative suffix under any enclosing header).
    /// `prefix`  is the full key prefix — used for workspace lookups and edits.
    ///           For bundle headers this is just the bundle name (e.g. `"messages"`).
    ///           For within-bundle headers it is bundle-qualified (e.g. `"messages:app.confirm"`).
    /// `depth`   drives indentation.
    Header { display: String, prefix: String, depth: usize },
    /// A selectable translation row.
    /// `display` is the text shown (".suffix" when under a header/key-parent, full key otherwise).
    /// `full_key` is the complete key — bundle-qualified when bundles are present
    ///            (e.g. `"messages:app.title"`), bare otherwise (e.g. `"app.title"`).
    /// `depth`    drives indentation.
    Key { display: String, full_key: String, depth: usize },
}


// ── Hierarchical Render Model ─────────────────────────────────────────────────

/// A structural group in a `BundleModel`.  Produced by the group-merge scan over
/// sorted entries.  Branch groups (≥2 direct children) are rendered as header lines;
/// leaf groups are stored for navigation and structural highlighting but not shown.
///
/// A group's `label` may span multiple dot-joined segments after single-child chain
/// collapsing (e.g. `"detail.something"` after merging a single-child chain).
#[derive(Debug, Clone)]
pub struct Group {
    /// Segment label; extended in place during the scan for single-child chains.
    pub label: String,
    /// Real segment depth of the shallowest segment (0-based).
    pub depth: usize,
    /// Index of the first `Entry` in this group's range (inclusive).
    pub first_entry: usize,
    /// Index of the last `Entry` in this group's range (inclusive).
    pub last_entry: usize,
    /// True when this group has ≥2 direct child groups.
    pub is_branch: bool,
}

/// One locale cell inside an `Entry`.
#[derive(Debug, Clone)]
pub struct LocaleCell {
    /// Raw translation value, or `None` when the key is absent in this locale file.
    pub value: Option<String>,
    /// True when there is a pending (unsaved) write for this (key, locale) pair.
    pub is_dirty: bool,
}

/// One translatable key in a `BundleModel`.
#[derive(Debug, Clone)]
pub struct Entry {
    /// Key segments split on `'.'`, e.g. `["app", "title"]`.
    pub segments: Vec<String>,
    /// One cell per locale in the parent `BundleModel.locales`, in the same order.
    pub cells: Vec<LocaleCell>,
    /// True when this key has at least one unsaved change.
    pub is_dirty: bool,
    /// True when this key is permanently pinned by the user.
    pub is_pinned: bool,
    /// True when this key was surfaced temporarily by the `ChildrenAll` scope.
    pub is_temp_pinned: bool,
}

impl Entry {
    /// Dot-joined key path (no bundle prefix).
    pub fn real_key(&self) -> String {
        self.segments.join(".")
    }
}

/// All visible data for one bundle (one group of `.properties` files sharing the
/// same base name).  Entries are sorted alphabetically by their joined segment path.
/// Groups are derived from the sorted entry list via the group-merge scan and stored
/// as pure structural data — the renderer queries them freely.
#[derive(Debug, Clone)]
pub struct BundleModel {
    /// Bundle name (empty string for bare/legacy keys).
    pub name: String,
    /// Locale names in display order, matching the indices in each `Entry.cells`.
    pub locales: Vec<String>,
    /// All visible entries, sorted by their dot-joined key path.
    pub entries: Vec<Entry>,
    /// Structural groups derived via the group-merge scan.
    /// Keys are dot-joined prefix paths within the bundle (no bundle qualifier).
    pub groups: HashMap<String, Group>,
}

/// The complete hierarchical render model.  This is the single source of truth
/// for the renderer once Step 2 of the refactoring is complete.  Until then it
/// lives alongside `display_rows` and is rebuilt in parallel.
#[derive(Debug, Clone)]
pub struct RenderModel {
    pub bundles: Vec<BundleModel>,
}

/// Build a `RenderModel` from the workspace and the current filter/display state.
///
/// `filtered_keys`  — the bundle-qualified keys to include (after filter).
/// `always_bundles` — bundle names that must emit a `BundleModel` even when empty.
/// `visible_locales`— locale columns currently visible.
/// `dirty_keys`     — bundle-qualified keys with unsaved changes.
/// `dirty_cells`    — `(full_key, locale)` pairs with unsaved changes.
/// `pinned_keys`    — permanently pinned keys (bypass filter).
/// `temp_pins`      — temporarily surfaced keys (`ChildrenAll` scope).
pub fn build_render_model(
    ws: &workspace::Workspace,
    filtered_keys: &[String],
    always_bundles: &[String],
    visible_locales: &[String],
    dirty_keys: &HashSet<String>,
    dirty_cells: &HashSet<(String, String)>,
    pinned_keys: &HashSet<String>,
    temp_pins: &[String],
) -> RenderModel {
    // Group keys by bundle name.  `BTreeMap` keeps bundles in alphabetical order,
    // matching the order produced by `build_display_rows`.
    let mut by_bundle: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for key in filtered_keys {
        match key.find(':') {
            Some(idx) => {
                by_bundle
                    .entry(key[..idx].to_string())
                    .or_default()
                    .push(key[idx + 1..].to_string());
            }
            None => {
                by_bundle.entry(String::new()).or_default().push(key.clone());
            }
        }
    }
    for bundle in always_bundles {
        by_bundle.entry(bundle.clone()).or_default();
    }

    let mut bundles: Vec<BundleModel> = Vec::new();

    for (bundle, real_keys) in &by_bundle {
        // Locales this bundle can provide (intersect visible_locales with what the
        // bundle actually has; bare-key bundles accept all visible locales).
        let bundle_locales: Vec<String> = visible_locales
            .iter()
            .filter(|l| ws.bundle_has_locale(bundle, l))
            .cloned()
            .collect();

        // Sort real keys within the bundle (they are already globally sorted when
        // coming from `merged_keys`, but sort explicitly for correctness).
        let mut sorted_keys = real_keys.clone();
        sorted_keys.sort();

        let entries: Vec<Entry> = sorted_keys
            .iter()
            .map(|real_key| {
                let full_key = if bundle.is_empty() {
                    real_key.clone()
                } else {
                    format!("{bundle}:{real_key}")
                };
                let segments: Vec<String> =
                    real_key.split('.').map(|s| s.to_string()).collect();
                let cells: Vec<LocaleCell> = bundle_locales
                    .iter()
                    .map(|locale| {
                        let value = ws.get_value(&full_key, locale).map(|v| v.to_string());
                        let is_dirty =
                            dirty_cells.contains(&(full_key.clone(), locale.clone()));
                        LocaleCell { value, is_dirty }
                    })
                    .collect();
                Entry {
                    segments,
                    cells,
                    is_dirty: dirty_keys.contains(&full_key),
                    is_pinned: pinned_keys.contains(&full_key),
                    is_temp_pinned: temp_pins.iter().any(|k| k == &full_key),
                }
            })
            .collect();

        let groups = build_groups(&entries);

        bundles.push(BundleModel {
            name: bundle.clone(),
            locales: bundle_locales,
            entries,
            groups,
        });
    }

    RenderModel { bundles }
}

/// Single-forward-pass group-merge scan over a sorted entry list.
///
/// Detects which prefix paths form structural groups and which of those are
/// single-child chains (collapsible into a merged label).  Returns a map from
/// dot-joined prefix path → `Group`.  Both the original prefix and any merged
/// child prefix map to the same `Group` object after a chain collapse.
///
/// See `docs/group-merge.md` for the full algorithm description and worked example.
fn build_groups(entries: &[Entry]) -> HashMap<String, Group> {
    // Mutable build state for one group while it is being constructed.
    struct Bg {
        label: String,
        depth: usize,
        first: usize,
        last: usize,
        is_branch: bool,
        /// Index in `bg` of the group that absorbed this one (self-referential if not absorbed).
        canonical: usize,
    }

    // One slot on the stack of currently-open (not yet closed) groups.
    struct Open {
        idx: usize,            // index in `bg`
        prefix: String,        // dot-joined prefix at this depth
        children: Vec<usize>,  // bg indices of direct children (appended as they close)
    }

    let mut bg: Vec<Bg> = Vec::new();
    // Pairs of (prefix, bg_idx) collected as groups close; used to build the final map.
    let mut closed: Vec<(String, usize)> = Vec::new();
    let mut stack: Vec<Open> = Vec::new();

    let n = entries.len();

    // Process entries 0..n, then a sentinel (i == n, segs = []) that closes everything.
    for i in 0..=n {
        let segs: &[String] = if i < n { &entries[i].segments } else { &[] };
        let prev_segs: &[String] = if i > 0 { &entries[i - 1].segments } else { &[] };

        let shared = common_prefix_len(segs, prev_segs);

        // Close depths >= shared, deepest first.
        while stack.len() > shared {
            let open = stack.pop().unwrap();
            bg[open.idx].last = i.saturating_sub(1);

            if open.children.len() == 1 {
                let child_idx = open.children[0];
                // Extend-in-place if ranges match: this group had exactly one child
                // that spanned the same entries — a single-child chain.
                if bg[child_idx].first == bg[open.idx].first
                    && bg[child_idx].last == bg[open.idx].last
                {
                    let child_label = bg[child_idx].label.clone();
                    bg[open.idx].label.push('.');
                    bg[open.idx].label.push_str(&child_label);
                    // Inherit the child's branch status (it may have been a branch itself
                    // due to cascading merges from deeper single-child chains).
                    bg[open.idx].is_branch = bg[child_idx].is_branch;
                    // Redirect: child's canonical now points to this group.
                    bg[child_idx].canonical = open.idx;
                }
                // If ranges don't match the child had a different extent — no merge.
            } else if open.children.len() > 1 {
                bg[open.idx].is_branch = true;
            }

            // Register this group with its parent (if one is open).
            if let Some(parent) = stack.last_mut() {
                parent.children.push(open.idx);
            }

            closed.push((open.prefix, open.idx));
        }

        if i == n {
            break;
        }

        // Open new groups for depths `shared..segs.len()`.
        for d in shared..segs.len() {
            let prefix = segs[..=d].join(".");
            let idx = bg.len();
            bg.push(Bg {
                label: segs[d].clone(),
                depth: d,
                first: i,
                last: i,      // updated when this group closes
                is_branch: false,
                canonical: idx, // self initially
            });
            stack.push(Open { idx, prefix, children: Vec::new() });
        }
    }

    // Build the final map.  For each closed prefix, follow the canonical chain to
    // the surviving group object and record it.  Both original and merged prefixes
    // will point to the same `Group` data (the absorbing group's data).
    let mut map: HashMap<String, Group> = HashMap::new();
    for (prefix, idx) in closed {
        // Follow the canonical chain.
        let mut canon = idx;
        loop {
            let next = bg[canon].canonical;
            if next == canon { break; }
            canon = next;
        }
        let g = &bg[canon];
        map.entry(prefix).or_insert_with(|| Group {
            label: g.label.clone(),
            depth: g.depth,
            first_entry: g.first,
            last_entry: g.last,
            is_branch: g.is_branch,
        });
    }

    map
}

/// Number of leading elements shared between two slices.
fn common_prefix_len(a: &[String], b: &[String]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

// ── Display row derivation ────────────────────────────────────────────────────
//
// These functions replace the trie-based `build_display_rows` approach.
// They produce an identical `Vec<DisplayRow>` from the hierarchical `RenderModel`.

/// Derives the flat `Vec<DisplayRow>` from the hierarchical render model.
/// Produces the same row ordering and display strings as the old trie-based
/// `build_display_rows`, driven by the group-merge data instead of a trie walk.
pub fn display_rows_from_render_model(rm: &RenderModel) -> Vec<DisplayRow> {
    let mut rows = Vec::new();
    for bundle in &rm.bundles {
        rows_for_bundle(bundle, &mut rows);
    }
    rows
}

fn rows_for_bundle(bundle: &BundleModel, rows: &mut Vec<DisplayRow>) {
    let bundle_offset: usize;
    if bundle.name.is_empty() {
        bundle_offset = 0;
    } else {
        rows.push(DisplayRow::Header {
            display: bundle.name.clone(),
            prefix:  bundle.name.clone(),
            depth:   0,
        });
        bundle_offset = 1;
    }

    // Context stack: effective paths (bare, within-bundle) of the most recently
    // emitted headers/key-parents.  The top is the nearest enclosing context.
    let mut ctx_stack: Vec<String> = Vec::new();

    for (i, entry) in bundle.entries.iter().enumerate() {
        let rk = entry.real_key();

        // ── Branch group headers for this entry ──────────────────────────────
        let mut header_groups: Vec<(&str, &Group)> = bundle
            .groups
            .iter()
            .filter(|(prefix, g)| {
                g.first_entry == i
                    && g.is_branch
                    // Use only the canonical prefix (depth of prefix == group.depth).
                    && prefix.split('.').count() == g.depth + 1
                    // Skip when this group IS the entry's own position (key-parent).
                    && prefix.as_str() != rk
                    // Skip when the merged label terminates at the entry's key.
                    && g.label != rk
            })
            .map(|(p, g)| (p.as_str(), g))
            .collect();

        // Shallowest groups first (top-down visual order).
        header_groups.sort_by_key(|(_, g)| g.depth);

        for (prefix, group) in &header_groups {
            let ep = group_effective_path(prefix, &group.label);
            trim_ctx_to_ancestor(&mut ctx_stack, &ep);

            let display = relative_display(&ep, ctx_stack.last().map(|s| s.as_str()));
            let depth   = group_visual_depth(prefix, &bundle.groups) + bundle_offset;
            let qualified = qualify(&bundle.name, &ep);

            rows.push(DisplayRow::Header { display, prefix: qualified, depth });
            ctx_stack.push(ep);
        }

        // ── Entry (key) row ───────────────────────────────────────────────────
        trim_ctx_to_ancestor(&mut ctx_stack, &rk);
        let display  = relative_display(&rk, ctx_stack.last().map(|s| s.as_str()));
        let depth    = entry_visual_depth(&entry.segments, &bundle.groups) + bundle_offset;
        let full_key = qualify(&bundle.name, &rk);

        rows.push(DisplayRow::Key { display, full_key, depth });

        // If this entry has children, push it as context so the children render
        // as ".suffix" relative to it.
        if bundle.entries.get(i + 1)
            .map_or(false, |next| next.real_key().starts_with(&format!("{rk}.")))
        {
            ctx_stack.push(rk);
        }
    }
}

// ── Rendering helpers ─────────────────────────────────────────────────────────

/// The "effective path" a group represents after chain collapsing.
/// For an unmerged group at prefix "a.b.c" with label "c": returns "a.b.c".
/// For a merged group at prefix "a" (depth 0) with label "a.b": returns "a.b".
pub fn group_effective_path(prefix: &str, label: &str) -> String {
    let segs: Vec<&str> = prefix.split('.').collect();
    if segs.len() == 1 {
        // Depth-0 group: the label IS the full effective path.
        label.to_string()
    } else {
        // Build from the parent segments + the (possibly merged) label.
        let parent = segs[..segs.len() - 1].join(".");
        format!("{parent}.{label}")
    }
}

/// Display text for `path` relative to `context`.
/// When context is a proper prefix of path, returns `".{suffix}"`.
/// Otherwise returns the full path (no leading dot).
pub fn relative_display(path: &str, context: Option<&str>) -> String {
    match context {
        Some(ctx)
            if path.len() > ctx.len()
                && path.starts_with(ctx)
                && path.as_bytes().get(ctx.len()) == Some(&b'.') =>
        {
            format!(".{}", &path[ctx.len() + 1..])
        }
        _ => path.to_string(),
    }
}

/// Pops context stack entries that are NOT strict ancestor prefixes of `item_path`.
pub fn trim_ctx_to_ancestor(stack: &mut Vec<String>, item_path: &str) {
    while let Some(top) = stack.last() {
        if item_path.starts_with(&format!("{top}.")) {
            break;
        }
        stack.pop();
    }
}

/// Qualifies a bare key with a bundle prefix, or returns it unchanged for bare-key bundles.
pub fn qualify(bundle: &str, key: &str) -> String {
    if bundle.is_empty() { key.to_string() } else { format!("{bundle}:{key}") }
}

/// Total segment depths absorbed by merged ancestor groups above `prefix`.
/// Only canonical prefixes (those whose segment count == group.depth + 1) are counted
/// to avoid double-counting when both the original and merged alias are in the map.
fn absorbed_above(prefix: &str, groups: &HashMap<String, Group>) -> usize {
    let segs: Vec<&str> = prefix.split('.').collect();
    let mut total = 0usize;
    for d in 0..segs.len().saturating_sub(1) {
        let ancestor = segs[..=d].join(".");
        if let Some(g) = groups.get(&ancestor) {
            if ancestor.split('.').count() == g.depth + 1 {
                total += g.label.split('.').count().saturating_sub(1);
            }
        }
    }
    total
}

/// Visual (display) depth for a group at `prefix`.
/// visual = real_depth − absorbed_from_merged_ancestors.
pub fn group_visual_depth(prefix: &str, groups: &HashMap<String, Group>) -> usize {
    let real = prefix.split('.').count().saturating_sub(1);
    real.saturating_sub(absorbed_above(prefix, groups))
}

/// Visual depth for an entry with the given segments.
/// visual = (segments.len() − 1) − absorbed_from_merged_ancestors.
pub fn entry_visual_depth(segments: &[String], groups: &HashMap<String, Group>) -> usize {
    if segments.is_empty() { return 0; }
    let real_key = segments.join(".");
    let real = segments.len() - 1;
    real.saturating_sub(absorbed_above(&real_key, groups))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(ks: &[&str]) -> Vec<String> {
        ks.iter().map(|s| s.to_string()).collect()
    }

    /// Build display rows from a list of bare keys (no bundle prefix).
    /// Replaces the old `build_display_rows(&keys, &[])` call in tests.
    fn rows_from_bare_keys(ks: &[&str]) -> Vec<DisplayRow> {
        let mut sorted: Vec<String> = ks.iter().map(|s| s.to_string()).collect();
        sorted.sort();
        let entries: Vec<Entry> = sorted.iter().map(|k| Entry {
            segments: k.split('.').map(|s| s.to_string()).collect(),
            cells: vec![],
            is_dirty: false,
            is_pinned: false,
            is_temp_pinned: false,
        }).collect();
        let groups = build_groups(&entries);
        let rm = RenderModel { bundles: vec![BundleModel {
            name: String::new(),
            locales: vec![],
            entries,
            groups,
        }]};
        display_rows_from_render_model(&rm)
    }

    fn headers(rows: &[DisplayRow]) -> Vec<&str> {
        rows.iter().filter_map(|r| match r {
            DisplayRow::Header { prefix, .. } => Some(prefix.as_str()),
            _ => None,
        }).collect()
    }

    fn displays(rows: &[DisplayRow]) -> Vec<&str> {
        rows.iter().map(|r| match r {
            DisplayRow::Header { display, .. } | DisplayRow::Key { display, .. } => display.as_str(),
        }).collect()
    }

    fn depths(rows: &[DisplayRow]) -> Vec<usize> {
        rows.iter().map(|r| match r {
            DisplayRow::Header { depth, .. } | DisplayRow::Key { depth, .. } => *depth,
        }).collect()
    }

    #[test]
    fn siblings_get_a_header() {
        // app → confirm (single-child non-key chain) collapses into "app.confirm" header.
        let rows = rows_from_bare_keys(&["app.confirm.delete", "app.confirm.discard"]);
        assert_eq!(headers(&rows), vec!["app.confirm"]);
        assert_eq!(displays(&rows), vec!["app.confirm", ".delete", ".discard"]);
        assert_eq!(depths(&rows),   vec![0,              1,         1]);
    }

    #[test]
    fn lone_key_chain_collapses() {
        // app → loading(key): chain absorbs the key — emits one Key row "app.loading" at depth 0,
        // no wrapping Header for the intermediate "app" node.
        let rows = rows_from_bare_keys(&["app.loading"]);
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], DisplayRow::Key { full_key, depth, .. }
            if full_key == "app.loading" && *depth == 0));
    }

    #[test]
    fn truly_bare_key_has_no_header() {
        // A key with no dots has no intermediate node — no Header emitted.
        let rows = rows_from_bare_keys(&["loading"]);
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], DisplayRow::Key { full_key, depth, .. }
            if full_key == "loading" && *depth == 0));
    }

    #[test]
    fn single_child_non_key_chain_collapses() {
        // com → myapp → error is a chain of single-child non-key nodes; error has ≥2 children
        // so the chain collapses into one "com.myapp.error" header.
        let rows = rows_from_bare_keys(&[
            "com.myapp.error.notfound",
            "com.myapp.error.timeout",
        ]);
        assert_eq!(headers(&rows), vec!["com.myapp.error"]);
        assert_eq!(displays(&rows), vec!["com.myapp.error", ".notfound", ".timeout"]);
        assert_eq!(depths(&rows),   vec![0,                  1,           1]);
    }

    #[test]
    fn mixed_depth_siblings_share_parent_header() {
        // app has two children (confirm, loading) so the chain doesn't collapse —
        // app gets its own Header. confirm → [delete, discard] is a single-child chain
        // but confirm has 2 key-children so no further collapsing there either.
        let rows = rows_from_bare_keys(&[
            "app.confirm.delete",
            "app.confirm.discard",
            "app.loading",
        ]);
        assert_eq!(headers(&rows), vec!["app", "app.confirm"]);
        assert_eq!(displays(&rows), vec!["app", ".confirm", ".delete", ".discard", ".loading"]);
        assert_eq!(depths(&rows),   vec![0,      1,          2,         2,          1]);
    }

    #[test]
    fn key_parent_no_duplicate_header() {
        // app → x(key-and-parent): chain absorbs x — emits Key "app.x" at depth 0 (no Header).
        // x's children .a and .b appear at depth 1.
        let rows = rows_from_bare_keys(&["app.x", "app.x.a", "app.x.b"]);
        assert!(headers(&rows).is_empty(), "app.x is a key — no Header row expected");
        assert_eq!(displays(&rows), vec!["app.x", ".a", ".b"]);
        assert_eq!(depths(&rows),   vec![0,        1,    1]);
    }

    #[test]
    fn key_parent_nested_under_header() {
        // com → err is a single-child non-key chain; err has 2 children → collapses to "com.err".
        // com.err.timeout is a key AND a parent; .deeper is its child.
        // BTreeMap order: other < timeout.
        // Expected:
        //   com.err:    Header depth 0
        //     .other    Key    depth 1
        //     .timeout  Key    depth 1  (key-and-parent)
        //       .deeper Key    depth 2
        let rows = rows_from_bare_keys(&[
            "com.err.other",
            "com.err.timeout",
            "com.err.timeout.deeper",
        ]);
        assert_eq!(headers(&rows), vec!["com.err"]);
        assert_eq!(displays(&rows), vec!["com.err", ".other", ".timeout", ".deeper"]);
        assert_eq!(depths(&rows),   vec![0,          1,        1,          2]);
    }

    #[test]
    fn nested_headers() {
        // app → a is a single-child non-key chain; a has 3 children → collapses to "app.a".
        // a.b has 2 children → gets its own ".b" header (relative to app.a context).
        let rows = rows_from_bare_keys(&[
            "app.a.b.c",
            "app.a.b.d",
            "app.a.e",
            "app.a.f",
        ]);
        assert_eq!(headers(&rows), vec!["app.a", "app.a.b"]);
        assert!(matches!(&rows[1], DisplayRow::Header { display, depth, .. }
            if display == ".b" && *depth == 1));
        assert_eq!(displays(&rows), vec!["app.a", ".b", ".c", ".d", ".e", ".f"]);
        assert_eq!(depths(&rows),   vec![0,         1,    2,    2,    1,    1]);
    }

    #[test]
    fn non_key_single_child_absorbed_into_key_display() {
        // com → err collapses to Header "com.err" (err has ≥2 children).
        // Within err, timeout is a non-key single-child node whose sole child IS a key:
        // the chain absorbs it → Key ".timeout.deeper" (no separate ".timeout" header).
        let rows = rows_from_bare_keys(&[
            "com.err.other",
            "com.err.second",
            "com.err.timeout.deeper",
        ]);
        assert_eq!(headers(&rows), vec!["com.err"]);
        assert_eq!(displays(&rows), vec!["com.err", ".other", ".second", ".timeout.deeper"]);
        assert_eq!(depths(&rows),   vec![0,          1,        1,         1]);
    }

    // ── Group-merge scan tests ────────────────────────────────────────────────

    fn segs(key: &str) -> Vec<String> {
        key.split('.').map(|s| s.to_string()).collect()
    }

    fn dummy_entry(key: &str) -> Entry {
        Entry {
            segments: segs(key),
            cells: vec![],
            is_dirty: false,
            is_pinned: false,
            is_temp_pinned: false,
        }
    }

    fn entries(keys: &[&str]) -> Vec<Entry> {
        keys.iter().map(|k| dummy_entry(k)).collect()
    }

    #[test]
    fn no_entries_gives_empty_groups() {
        let groups = build_groups(&[]);
        assert!(groups.is_empty());
    }

    #[test]
    fn single_entry_chain_collapses() {
        // "http.status" → "http" has exactly one child "status" with the same range
        // → single-child chain collapsed: both prefixes point to the merged group
        //   with label "http.status".  is_branch = false (leaf chain).
        let groups = build_groups(&entries(&["http.status"]));
        assert_eq!(groups.len(), 2);
        let http = groups.get("http").expect("http group missing");
        assert!(!http.is_branch);
        assert_eq!(http.label, "http.status");
        let status = groups.get("http.status").expect("http.status alias missing");
        assert!(!status.is_branch);
        assert_eq!(status.label, "http.status");
        assert_eq!(status.first_entry, http.first_entry);
    }

    #[test]
    fn two_siblings_make_parent_branch() {
        // "a.x" and "a.y" → "a" is a branch (2 children: x, y).
        let groups = build_groups(&entries(&["a.x", "a.y"]));
        let a = groups.get("a").expect("a group missing");
        assert!(a.is_branch);
        assert_eq!(a.label, "a");
        assert_eq!(a.first_entry, 0);
        assert_eq!(a.last_entry, 1);
    }

    #[test]
    fn single_child_chain_collapses() {
        // "detail" → "something" → [two children] should collapse into "detail.something".
        let es = entries(&[
            "detail.something.msg.first",
            "detail.something.msg.second",
            "detail.something.notmsg",
        ]);
        let groups = build_groups(&es);
        // "detail" had one child "something" and same range → merged to "detail.something".
        let g = groups.get("detail").expect("detail group missing");
        assert_eq!(g.label, "detail.something");
        assert!(g.is_branch);
        assert_eq!(g.first_entry, 0);
        assert_eq!(g.last_entry, 2);
        // "detail.something" also maps to the same group.
        let g2 = groups.get("detail.something").expect("detail.something missing");
        assert_eq!(g2.label, "detail.something");
        assert_eq!(g2.first_entry, g.first_entry);
        // "msg" at depth 2 has two children → branch, no merge.
        let msg = groups.get("detail.something.msg").expect("msg missing");
        assert_eq!(msg.label, "msg");
        assert!(msg.is_branch);
    }

    #[test]
    fn worked_example_detail_something() {
        // The worked example from docs/group-merge.md (bundle `stripping`):
        //   0:  [http, something]
        //   1:  [http, status]
        //   2:  [http, status, 200]
        //   3:  [http, status, 400]
        //   4:  [http, status, 401]
        //   5:  [http, status, 403]
        //   6:  [http, status, 404]
        //   7:  [http, status, 500]
        //   8:  [http, status, detail, something, msg, firstmessage]
        //   9:  [http, status, detail, something, msg, secondmessage]
        //   10: [http, status, detail, something, notmsg]
        //   11: [http, status, x]
        //   12: [http, y]
        let es = entries(&[
            "http.something",
            "http.status",
            "http.status.200",
            "http.status.400",
            "http.status.401",
            "http.status.403",
            "http.status.404",
            "http.status.500",
            "http.status.detail.something.msg.firstmessage",
            "http.status.detail.something.msg.secondmessage",
            "http.status.detail.something.notmsg",
            "http.status.x",
            "http.y",
        ]);
        let groups = build_groups(&es);

        // "http" spans all 13 entries, branch (something + status + y).
        let http = groups.get("http").expect("http");
        assert!(http.is_branch);
        assert_eq!(http.first_entry, 0);
        assert_eq!(http.last_entry, 12);

        // "http.status" spans entries 1–11 and is a branch.
        let status = groups.get("http.status").expect("http.status");
        assert!(status.is_branch);
        assert_eq!(status.first_entry, 1);
        assert_eq!(status.last_entry, 11);

        // "http.status.detail" had one child "something" with matching range 8–10
        // → merged to "detail.something".
        let detail = groups.get("http.status.detail").expect("http.status.detail");
        assert_eq!(detail.label, "detail.something");
        assert_eq!(detail.first_entry, 8);
        assert_eq!(detail.last_entry, 10);
        // Both prefixes resolve to the same group data.
        let ds = groups.get("http.status.detail.something").expect("detail.something alias");
        assert_eq!(ds.label, "detail.something");
        assert_eq!(ds.first_entry, 8);

        // "msg" at depth 4 has two children (firstmessage, secondmessage) → branch.
        let msg = groups.get("http.status.detail.something.msg").expect("msg");
        assert_eq!(msg.label, "msg");
        assert!(msg.is_branch);
        assert_eq!(msg.first_entry, 8);
        assert_eq!(msg.last_entry, 9);
    }
}
