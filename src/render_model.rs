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
pub fn build_display_rows(keys: &[String]) -> Vec<DisplayRow> {
    let has_bundles = keys.iter().any(|k| k.contains(':'));
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

    // How many immediate children are keys (leaf or key-with-children)?
    let key_child_count = node.children.values().filter(|c| c.is_key).count();

    if !node.is_key && key_child_count >= 2 {
        // Non-key branch with ≥2 key-children → emit a Header row and recurse
        // children at the next indent level, using this node as the new context.
        rows.push(DisplayRow::Header {
            display: display.clone(),
            prefix: full_key.clone(),
            depth,
        });
        for (seg, child) in &node.children {
            walk(child, &format!("{seg_path}.{seg}"), depth + 1, Some(seg_path), key_prefix, rows);
        }
    } else if node.is_key {
        // Key that also has children: already emitted as Key above.
        // Recurse children at depth+1, using this key as the new display context.
        for (seg, child) in &node.children {
            walk(child, &format!("{seg_path}.{seg}"), depth + 1, Some(seg_path), key_prefix, rows);
        }
    } else {
        // Single-chain or non-qualifying branch: pass depth and context through
        // unchanged so children display their full relative path from the outer header.
        for (seg, child) in &node.children {
            walk(child, &format!("{seg_path}.{seg}"), depth, header_prefix, key_prefix, rows);
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
        let rows = build_display_rows(&keys(&["app.confirm.delete", "app.confirm.discard"]));
        assert!(matches!(&rows[0], DisplayRow::Header { prefix, .. } if prefix == "app.confirm"));
        assert!(matches!(&rows[1], DisplayRow::Key { display, .. } if display == ".delete"));
        assert!(matches!(&rows[2], DisplayRow::Key { display, .. } if display == ".discard"));
    }

    #[test]
    fn lone_key_has_no_header() {
        let rows = build_display_rows(&keys(&["app.loading"]));
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], DisplayRow::Key { full_key, depth, .. }
            if full_key == "app.loading" && *depth == 0));
    }

    #[test]
    fn single_child_chain_collapses() {
        let rows = build_display_rows(&keys(&[
            "com.myapp.error.notfound",
            "com.myapp.error.timeout",
        ]));
        assert!(matches!(&rows[0], DisplayRow::Header { prefix, depth, .. }
            if prefix == "com.myapp.error" && *depth == 0));
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn mixed_depth_siblings_no_shared_header() {
        // app.confirm.* gets a header; app.loading stands alone — no "app:" header.
        let rows = build_display_rows(&keys(&[
            "app.confirm.delete",
            "app.confirm.discard",
            "app.loading",
        ]));
        assert_eq!(headers(&rows), vec!["app.confirm"]);
    }

    #[test]
    fn key_parent_no_duplicate_header() {
        // app.x is both a key and a namespace parent.
        // It should appear as exactly one Key row (no separate Header row).
        let rows = build_display_rows(&keys(&["app.x", "app.x.a", "app.x.b"]));
        let header_prefixes = headers(&rows);
        assert!(!header_prefixes.contains(&"app.x"), "app.x is a key — no Header row expected");
        // Key row for app.x at depth 0, children at depth 1.
        assert!(matches!(&rows[0], DisplayRow::Key { full_key, depth, .. }
            if full_key == "app.x" && *depth == 0));
        assert_eq!(depths(&rows), vec![0, 1, 1]);
    }

    #[test]
    fn key_parent_nested_under_header() {
        // com.err.timeout is a key AND has a child .deeper.
        // BTreeMap sorts alphabetically so .other comes before .timeout.
        // Expected:
        //   com.err:          Header depth 0
        //     .other          Key    depth 1
        //     .timeout        Key    depth 1
        //       .deeper       Key    depth 2
        let rows = build_display_rows(&keys(&[
            "com.err.other",
            "com.err.timeout",
            "com.err.timeout.deeper",
        ]));
        assert_eq!(headers(&rows), vec!["com.err"]);
        assert_eq!(displays(&rows), vec!["com.err", ".other", ".timeout", ".deeper"]);
        assert_eq!(depths(&rows),   vec![0,         1,        1,          2]);
    }

    #[test]
    fn nested_headers() {
        // app.a has two key-children (.e, .f) and one non-key sub-group (.b).
        // app.a should become a Header; app.a.b should become a nested Header.
        let rows = build_display_rows(&keys(&[
            "app.a.b.c",
            "app.a.b.d",
            "app.a.e",
            "app.a.f",
        ]));
        assert_eq!(headers(&rows), vec!["app.a", "app.a.b"]);
        // app.a.b header should display as ".b" (relative to app.a)
        assert!(matches!(&rows[1], DisplayRow::Header { display, depth, .. }
            if display == ".b" && *depth == 1));
        assert_eq!(depths(&rows), vec![0, 1, 2, 2, 1, 1]);
    }

    #[test]
    fn non_key_single_chain_shows_full_relative_path() {
        // com.err.timeout.deeper but timeout is NOT a key.
        // A header forms because com.err has ≥2 key-children (other, second).
        // timeout is a single-chain non-key pass-through, so deeper inherits the
        // com.err header context and displays as ".timeout.deeper".
        let rows = build_display_rows(&keys(&[
            "com.err.other",
            "com.err.second",
            "com.err.timeout.deeper",
        ]));
        assert_eq!(headers(&rows), vec!["com.err"]);
        // BTreeMap order: other, second, timeout.deeper (t > s > o alphabetically: no, o < s < t)
        assert_eq!(displays(&rows), vec!["com.err", ".other", ".second", ".timeout.deeper"]);
        assert_eq!(depths(&rows),   vec![0,          1,        1,         1]);
    }
}
