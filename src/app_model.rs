use std::collections::{BTreeMap, HashSet};
use crate::{radix_tree_arena::CompressedTrie, workspace};
// BTreeMap is used only in build_app_model (to keep bundles in alphabetical order).



// ── App Model (domain model) ──────────────────────────────────────────────────

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
    /// True when this key is in `merged_keys` but has no value in any locale file
    /// (i.e. it was created this session and not yet saved to disk).
    pub is_dangling: bool,
}

impl Entry {
    /// Dot-joined key path (no bundle prefix).
    pub fn real_key(&self) -> String {
        self.segments.join(".")
    }
}

/// All visible data for one bundle (one group of `.properties` files sharing the
/// same base name).  Entries are sorted alphabetically by their joined segment path.
/// The trie is an auxiliary structural index for rendering and navigation queries.
#[derive(Debug)]
pub struct BundleModel {
    /// Bundle name (empty string for bare/legacy keys).
    pub name: String,
    /// Locale names in display order, matching the indices in each `Entry.cells`.
    pub locales: Vec<String>,
    /// All visible entries, sorted by their dot-joined key path.
    pub entries: Vec<Entry>,
    /// Compressed radix trie over entry keys; value = entry index in `entries`.
    /// Used to answer structural questions: is a prefix a branch? has children?
    pub trie: CompressedTrie<usize>,
}

/// The complete hierarchical app model.  Single source of truth for navigation,
/// filter evaluation, and rendering.  Rebuilt on every filter/state change.
#[derive(Debug)]
pub struct DomainModel {
    pub bundles: Vec<BundleModel>,
}


/// Build a `DomainModel` from the workspace and the current filter/display state.
///
/// `filtered_keys`  — the bundle-qualified keys to include (after filter).
/// `always_bundles` — bundle names that must emit a `BundleModel` even when empty.
/// `visible_locales`— locale columns currently visible.
/// `dirty_keys`     — bundle-qualified keys with unsaved changes.
/// `dirty_cells`    — `(full_key, locale)` pairs with unsaved changes.
/// `pinned_keys`    — permanently pinned keys (bypass filter).
/// `temp_pins`      — temporarily surfaced keys (`ChildrenAll` scope).
pub fn build_domain_model(
    ws: &workspace::Workspace,
    filtered_keys: &[String],
    always_bundles: &[String],
    visible_locales: &[String],
    dirty_keys: &HashSet<String>,
    dirty_cells: &HashSet<(String, String)>,
    pinned_keys: &HashSet<String>,
    temp_pins: &[String],
) -> DomainModel {
    // Group keys by bundle name.  `BTreeMap` keeps bundles in alphabetical order,
    // matching the order produced by `build_view_rows`.
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
                    is_dangling: ws.is_dangling(&full_key),
                }
            })
            .collect();

        // Build the structural trie: insert real_key → entry_index for each entry.
        let mut trie = CompressedTrie::new();
        for (i, entry) in entries.iter().enumerate() {
            trie.insert_str(&entry.real_key(), i);
        }

        bundles.push(BundleModel {
            name: bundle.clone(),
            locales: bundle_locales,
            entries,
            trie,
        });
    }

    DomainModel { bundles }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Qualifies a bare key with a bundle prefix, or returns it unchanged for bare-key bundles.
pub fn qualify(bundle: &str, key: &str) -> String {
    if bundle.is_empty() { key.to_string() } else { format!("{bundle}:{key}") }
}

/// Number of leading elements shared between two slices.
pub(crate) fn common_prefix_len(a: &[String], b: &[String]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test-only display row type ──────────────────────────────────────────────

    enum TestRow {
        Header { display: String, prefix: String, depth: usize },
        Key    { display: String, full_key: String, depth: usize },
    }

    /// Build display rows from a list of bare keys (no bundle prefix) using the
    /// trie + consecutive-comparison approach.
    ///
    /// `display` = partition segments joined; leading "." when gi > 0.
    /// `depth`   = gi (partition index = visual indentation level).
    /// `prefix`  = dot-joined path through the current partition depth (headers only).
    fn rows_from_bare_keys(ks: &[&str]) -> Vec<TestRow> {
        let mut sorted: Vec<String> = ks.iter().map(|s| s.to_string()).collect();
        sorted.sort();
        let entries: Vec<Entry> = sorted.iter().map(|k| Entry {
            segments: k.split('.').map(|s| s.to_string()).collect(),
            cells: vec![],
            is_dirty: false,
            is_pinned: false,
            is_temp_pinned: false,
            is_dangling: false,
        }).collect();

        let mut trie = CompressedTrie::new();
        for (i, entry) in entries.iter().enumerate() {
            trie.insert_str(&entry.real_key(), i);
        }

        let mut rows: Vec<TestRow> = Vec::new();
        let mut prev_segs: &[String] = &[];

        for entry in &entries {
            let segs = &entry.segments;
            let shared = common_prefix_len(segs, prev_segs);
            let full_key = segs.join(".");

            let mut prev_end = 0usize;
            let mut gi = 0usize;

            for d in 0..segs.len().saturating_sub(1) {
                let path: Vec<&str> = segs[..=d].iter().map(|s| s.as_str()).collect();
                if trie.is_render_boundary_at(&path) {
                    if d >= shared {
                        // This prefix is newly introduced at this entry → header row.
                        let part_segs = &segs[prev_end..=d];
                        let text = part_segs.join(".");
                        let display = if gi == 0 { text } else { format!(".{}", text) };
                        let prefix = segs[..=d].join(".");
                        rows.push(TestRow::Header { display, prefix, depth: gi });
                    }
                    prev_end = d + 1;
                    gi += 1;
                }
            }
            // Leaf partition — always introduced.
            let part_segs = &segs[prev_end..];
            let text = part_segs.join(".");
            let display = if gi == 0 { text } else { format!(".{}", text) };
            rows.push(TestRow::Key { display, full_key, depth: gi });

            prev_segs = segs;
        }
        rows
    }

    fn headers(rows: &[TestRow]) -> Vec<&str> {
        rows.iter().filter_map(|r| match r {
            TestRow::Header { prefix, .. } => Some(prefix.as_str()),
            _ => None,
        }).collect()
    }

    fn displays(rows: &[TestRow]) -> Vec<&str> {
        rows.iter().map(|r| match r {
            TestRow::Header { display, .. } | TestRow::Key { display, .. } => display.as_str(),
        }).collect()
    }

    fn depths(rows: &[TestRow]) -> Vec<usize> {
        rows.iter().map(|r| match r {
            TestRow::Header { depth, .. } | TestRow::Key { depth, .. } => *depth,
        }).collect()
    }

    #[test]
    fn siblings_get_a_header() {
        // app.confirm has 2 children — branch node.  Single-child chain app→confirm
        // is compressed in the trie: no branch at ["app"], so no extra indirection.
        let rows = rows_from_bare_keys(&["app.confirm.delete", "app.confirm.discard"]);
        assert_eq!(headers(&rows), vec!["app.confirm"]);
        assert_eq!(displays(&rows), vec!["app.confirm", ".delete", ".discard"]);
        assert_eq!(depths(&rows),   vec![0,              1,         1]);
    }

    #[test]
    fn lone_key_chain_collapses() {
        // Single entry — no branch anywhere; emits one Key row at depth 0.
        let rows = rows_from_bare_keys(&["app.loading"]);
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], TestRow::Key { full_key, depth, .. }
            if full_key == "app.loading" && *depth == 0));
    }

    #[test]
    fn truly_bare_key_has_no_header() {
        let rows = rows_from_bare_keys(&["loading"]);
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], TestRow::Key { full_key, depth, .. }
            if full_key == "loading" && *depth == 0));
    }

    #[test]
    fn single_child_non_key_chain_collapses() {
        // com→myapp→error chain-collapses to one "com.myapp.error" header.
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
        // app.x is a key AND has 2 children (Interior, child_count=2).
        // is_render_boundary_at(["app","x"]) = true, but depth 1 is always within
        // `shared` when processing descendants, so no header row is introduced.
        let rows = rows_from_bare_keys(&["app.x", "app.x.a", "app.x.b"]);
        assert!(headers(&rows).is_empty(), "app.x is a key — no Header row expected");
        assert_eq!(displays(&rows), vec!["app.x", ".a", ".b"]);
        assert_eq!(depths(&rows),   vec![0,        1,    1]);
    }

    #[test]
    fn key_parent_nested_under_header() {
        // com.err has 2 children (other, timeout) — branch.
        // com.err.timeout is a key AND a parent (Interior, 1 child "deeper").
        // Children of Interior nodes appear at depth+1 even with 1 child.
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
        let rows = rows_from_bare_keys(&[
            "app.a.b.c",
            "app.a.b.d",
            "app.a.e",
            "app.a.f",
        ]);
        assert_eq!(headers(&rows), vec!["app.a", "app.a.b"]);
        assert!(matches!(&rows[1], TestRow::Header { display, depth, .. }
            if display == ".b" && *depth == 1));
        assert_eq!(displays(&rows), vec!["app.a", ".b", ".c", ".d", ".e", ".f"]);
        assert_eq!(depths(&rows),   vec![0,         1,    2,    2,    1,    1]);
    }

    #[test]
    fn non_key_single_child_absorbed_into_key_display() {
        // timeout (non-key, 1 child) is chain-collapsed; sole child is the key.
        // Display: ".timeout.deeper" at depth 1 (single-segment partition absorbed).
        let rows = rows_from_bare_keys(&[
            "com.err.other",
            "com.err.second",
            "com.err.timeout.deeper",
        ]);
        assert_eq!(headers(&rows), vec!["com.err"]);
        assert_eq!(displays(&rows), vec!["com.err", ".other", ".second", ".timeout.deeper"]);
        assert_eq!(depths(&rows),   vec![0,          1,        1,         1]);
    }
}
