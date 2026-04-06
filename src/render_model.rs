use std::collections::BTreeMap;

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

/// Converts a sorted, flat key list into a flat sequence of header and key rows.
///
/// When any key contains `:`, keys are grouped by bundle (the part before `:`).
/// Each bundle emits a `Header` row at depth 0, and its keys are walked at depth 1.
/// Keys without `:` are walked at depth 0 (backward-compatible legacy path).
///
/// Within each bundle the trie walk applies the same rules as before:
/// - A non-key node emits a `Header` when ≥2 of its immediate children are keys.
/// - A key-node that also has children emits only a `Key` row; children appear indented below.
/// - Single-child chains forward depth and display context unchanged.
/// `always_bundles` lists bundle names that should always emit a header row even
/// when no filtered keys belong to them.  Pass `workspace.bundle_names()` here
/// so bundle headers survive aggressive filters like `-/`.
pub fn build_display_rows(keys: &[String], always_bundles: &[String]) -> Vec<DisplayRow> {
    let has_bundles = keys.iter().any(|k| k.contains(':')) || !always_bundles.is_empty();
    let mut rows = Vec::new();

    if !has_bundles {
        // Legacy path: all keys are bare (no bundle prefix).
        let root = build_trie(keys);
        for (seg, child) in &root.children {
            walk(child, seg, 0, None, "", &mut rows);
        }
        return rows;
    }

    // Group keys by bundle name (part before ':').
    let mut by_bundle: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for key in keys {
        match key.find(':') {
            Some(idx) => {
                by_bundle
                    .entry(key[..idx].to_string())
                    .or_default()
                    .push(key[idx + 1..].to_string());
            }
            None => {
                // Bare key mixed in with bundle keys — group under empty bundle.
                by_bundle.entry(String::new()).or_default().push(key.clone());
            }
        }
    }
    // Ensure every always-bundle has an entry (possibly empty) so its header appears.
    for bundle in always_bundles {
        by_bundle.entry(bundle.clone()).or_default();
    }

    for (bundle, real_keys) in &by_bundle {
        if bundle.is_empty() {
            let root = build_trie(real_keys);
            for (seg, child) in &root.children {
                walk(child, seg, 0, None, "", &mut rows);
            }
        } else {
            // Bundle header at depth 0.
            rows.push(DisplayRow::Header {
                display: bundle.clone(),
                prefix: bundle.clone(), // bundle name only — not a real key path
                depth: 0,
            });
            let root = build_trie(real_keys);
            // `key_prefix` is prepended to seg_path to form the full_key stored in rows.
            let key_prefix = format!("{bundle}:");
            for (seg, child) in &root.children {
                walk(child, seg, 1, None, &key_prefix, &mut rows);
            }
        }
    }
    rows
}

// ── Trie ─────────────────────────────────────────────────────────────────────

struct TrieNode {
    children: BTreeMap<String, TrieNode>, // BTreeMap keeps siblings in sorted order
    is_key: bool,
}

impl TrieNode {
    fn new() -> Self {
        Self { children: BTreeMap::new(), is_key: false }
    }
}

fn build_trie(keys: &[String]) -> TrieNode {
    let mut root = TrieNode::new();
    for key in keys {
        let mut node = &mut root;
        for segment in key.split('.') {
            node = node.children
                .entry(segment.to_string())
                .or_insert_with(TrieNode::new);
        }
        node.is_key = true;
    }
    root
}

// ── Walk ─────────────────────────────────────────────────────────────────────

/// `seg_path`      — dot-joined path within the current bundle (no bundle prefix)
/// `depth`         — visual indentation level
/// `header_prefix` — `seg_path` of the nearest Header or key-parent already emitted;
///                   used to compute relative display text
/// `key_prefix`    — prepended to `seg_path` to form the `full_key` stored in rows;
///                   `""` for bare keys, `"bundle:"` for bundle-qualified keys
fn walk(node: &TrieNode, seg_path: &str, depth: usize, header_prefix: Option<&str>, key_prefix: &str, rows: &mut Vec<DisplayRow>) {
    // Display text: relative suffix from the nearest header/key-parent, or the full
    // seg_path when there is no enclosing context.
    let display = match header_prefix {
        Some(hp) => format!(".{}", &seg_path[hp.len() + 1..]),
        None => seg_path.to_string(),
    };
    // full_key used in DisplayRow — bundle-qualified when key_prefix is set.
    let full_key = format!("{key_prefix}{seg_path}");

    if node.is_key {
        rows.push(DisplayRow::Key {
            display: display.clone(),
            full_key: full_key.clone(),
            depth,
        });
    }

    if node.children.is_empty() {
        return;
    }

    if !node.is_key {
        // Collapse single-child chains into the fewest rows possible.
        // Walk forward through consecutive single-child nodes; stop when the node has
        // ≥2 children (branch point). We also absorb through one trailing key node —
        // so `http → status(key) → [200, 400, …]` becomes a single Key row
        // `http.status:` rather than a Header `http:` with a Key `.status` underneath.
        let mut chain_path = seg_path.to_string();
        let mut chain_node = node;
        while chain_node.children.len() == 1 {
            let (seg, child) = chain_node.children.iter().next().unwrap();
            chain_path = format!("{chain_path}.{seg}");
            chain_node = child;
            if chain_node.is_key { break; } // absorbed a key — stop here
        }
        let chain_display = match header_prefix {
            Some(hp) => format!(".{}", &chain_path[hp.len() + 1..]),
            None => chain_path.clone(),
        };
        let chain_full_key = format!("{key_prefix}{chain_path}");
        if chain_node.is_key {
            // Chain ended at a key: emit a Key row for the full collapsed path.
            rows.push(DisplayRow::Key {
                display: chain_display,
                full_key: chain_full_key,
                depth,
            });
        } else {
            // Chain ended at a branch point (≥2 children): emit a Header.
            rows.push(DisplayRow::Header {
                display: chain_display,
                prefix: chain_full_key,
                depth,
            });
        }
        for (seg, child) in &chain_node.children {
            walk(child, &format!("{chain_path}.{seg}"), depth + 1, Some(&chain_path), key_prefix, rows);
        }
    } else {
        // Key that also has children: already emitted as Key above.
        // Children indent one level under this key row.
        for (seg, child) in &node.children {
            walk(child, &format!("{seg_path}.{seg}"), depth + 1, Some(seg_path), key_prefix, rows);
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(ks: &[&str]) -> Vec<String> {
        ks.iter().map(|s| s.to_string()).collect()
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
        let rows = build_display_rows(&keys(&["app.confirm.delete", "app.confirm.discard"]), &[]);
        assert_eq!(headers(&rows), vec!["app.confirm"]);
        assert_eq!(displays(&rows), vec!["app.confirm", ".delete", ".discard"]);
        assert_eq!(depths(&rows),   vec![0,              1,         1]);
    }

    #[test]
    fn lone_key_chain_collapses() {
        // app → loading(key): chain absorbs the key — emits one Key row "app.loading" at depth 0,
        // no wrapping Header for the intermediate "app" node.
        let rows = build_display_rows(&keys(&["app.loading"]), &[]);
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], DisplayRow::Key { full_key, depth, .. }
            if full_key == "app.loading" && *depth == 0));
    }

    #[test]
    fn truly_bare_key_has_no_header() {
        // A key with no dots has no intermediate node — no Header emitted.
        let rows = build_display_rows(&keys(&["loading"]), &[]);
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], DisplayRow::Key { full_key, depth, .. }
            if full_key == "loading" && *depth == 0));
    }

    #[test]
    fn single_child_non_key_chain_collapses() {
        // com → myapp → error is a chain of single-child non-key nodes; error has ≥2 children
        // so the chain collapses into one "com.myapp.error" header.
        let rows = build_display_rows(&keys(&[
            "com.myapp.error.notfound",
            "com.myapp.error.timeout",
        ]), &[]);
        assert_eq!(headers(&rows), vec!["com.myapp.error"]);
        assert_eq!(displays(&rows), vec!["com.myapp.error", ".notfound", ".timeout"]);
        assert_eq!(depths(&rows),   vec![0,                  1,           1]);
    }

    #[test]
    fn mixed_depth_siblings_share_parent_header() {
        // app has two children (confirm, loading) so the chain doesn't collapse —
        // app gets its own Header. confirm → [delete, discard] is a single-child chain
        // but confirm has 2 key-children so no further collapsing there either.
        let rows = build_display_rows(&keys(&[
            "app.confirm.delete",
            "app.confirm.discard",
            "app.loading",
        ]), &[]);
        assert_eq!(headers(&rows), vec!["app", "app.confirm"]);
        assert_eq!(displays(&rows), vec!["app", ".confirm", ".delete", ".discard", ".loading"]);
        assert_eq!(depths(&rows),   vec![0,      1,          2,         2,          1]);
    }

    #[test]
    fn key_parent_no_duplicate_header() {
        // app → x(key-and-parent): chain absorbs x — emits Key "app.x" at depth 0 (no Header).
        // x's children .a and .b appear at depth 1.
        let rows = build_display_rows(&keys(&["app.x", "app.x.a", "app.x.b"]), &[]);
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
        let rows = build_display_rows(&keys(&[
            "com.err.other",
            "com.err.timeout",
            "com.err.timeout.deeper",
        ]), &[]);
        assert_eq!(headers(&rows), vec!["com.err"]);
        assert_eq!(displays(&rows), vec!["com.err", ".other", ".timeout", ".deeper"]);
        assert_eq!(depths(&rows),   vec![0,          1,        1,          2]);
    }

    #[test]
    fn nested_headers() {
        // app → a is a single-child non-key chain; a has 3 children → collapses to "app.a".
        // a.b has 2 children → gets its own ".b" header (relative to app.a context).
        let rows = build_display_rows(&keys(&[
            "app.a.b.c",
            "app.a.b.d",
            "app.a.e",
            "app.a.f",
        ]), &[]);
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
        let rows = build_display_rows(&keys(&[
            "com.err.other",
            "com.err.second",
            "com.err.timeout.deeper",
        ]), &[]);
        assert_eq!(headers(&rows), vec!["com.err"]);
        assert_eq!(displays(&rows), vec!["com.err", ".other", ".second", ".timeout.deeper"]);
        assert_eq!(depths(&rows),   vec![0,          1,        1,         1]);
    }
}
