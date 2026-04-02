use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub enum DisplayRow {
    /// A non-selectable group header, e.g. "com.myapp.error".
    Header { prefix: String },
    /// A selectable translation row.
    Key {
        /// Text shown in the key column.
        /// ".suffix" when under a header; the full key when standing alone.
        display: String,
        /// The complete key — used for workspace lookups and edits.
        full_key: String,
    },
}

/// Converts a sorted, flat key list into a flat sequence of header and key rows.
///
/// Rules:
/// - A `Header` is emitted for a trie node when *all* of its children are leaf keys
///   and there are ≥2 of them (the branch-point rule).
/// - Single-child chains collapse silently: "com.myapp.admin.user" never becomes a
///   header; the prefix accumulates until a qualifying branch is reached.
/// - Nodes with mixed-depth children (some leaf, some not) get no header; each
///   sub-tree is emitted independently at its own depth.
pub fn build_display_rows(keys: &[String]) -> Vec<DisplayRow> {
    let root = build_trie(keys);
    let mut rows = Vec::new();
    for (seg, child) in &root.children {
        walk(child, seg, &mut rows);
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

fn walk(node: &TrieNode, prefix: &str, rows: &mut Vec<DisplayRow>) {
    // If this node is itself a key (e.g. "app.confirm" exists alongside
    // "app.confirm.delete"), emit it before processing its children.
    if node.is_key {
        rows.push(DisplayRow::Key {
            display: prefix.to_string(),
            full_key: prefix.to_string(),
        });
    }

    if node.children.is_empty() {
        return;
    }

    let all_leaf_children = node.children.values().all(|c| c.children.is_empty());

    if all_leaf_children && node.children.len() >= 2 {
        // Branch point with all-leaf children → emit a header + one Key per child.
        rows.push(DisplayRow::Header { prefix: prefix.to_string() });
        for (seg, _child) in &node.children {
            rows.push(DisplayRow::Key {
                display: format!(".{seg}"),
                full_key: format!("{prefix}.{seg}"),
            });
        }
    } else {
        // Mixed depths or a single-child chain → recurse without emitting a header.
        for (seg, child) in &node.children {
            walk(child, &format!("{prefix}.{seg}"), rows);
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

    #[test]
    fn siblings_get_a_header() {
        let rows = build_display_rows(&keys(&["app.confirm.delete", "app.confirm.discard"]));
        assert!(matches!(&rows[0], DisplayRow::Header { prefix } if prefix == "app.confirm"));
        assert!(matches!(&rows[1], DisplayRow::Key { display, .. } if display == ".delete"));
        assert!(matches!(&rows[2], DisplayRow::Key { display, .. } if display == ".discard"));
    }

    #[test]
    fn lone_key_has_no_header() {
        let rows = build_display_rows(&keys(&["app.loading"]));
        assert_eq!(rows.len(), 1);
        assert!(matches!(&rows[0], DisplayRow::Key { full_key, .. } if full_key == "app.loading"));
    }

    #[test]
    fn single_child_chain_collapses() {
        // com → myapp → error → (notfound, timeout): header should be "com.myapp.error"
        let rows = build_display_rows(&keys(&[
            "com.myapp.error.notfound",
            "com.myapp.error.timeout",
        ]));
        assert!(matches!(&rows[0], DisplayRow::Header { prefix } if prefix == "com.myapp.error"));
    }

    #[test]
    fn mixed_depth_siblings_no_shared_header() {
        // app.confirm.* gets a header; app.loading stands alone — no "app:" header
        let rows = build_display_rows(&keys(&[
            "app.confirm.delete",
            "app.confirm.discard",
            "app.loading",
        ]));
        let header_prefixes: Vec<&str> = rows.iter().filter_map(|r| match r {
            DisplayRow::Header { prefix } => Some(prefix.as_str()),
            _ => None,
        }).collect();
        assert_eq!(header_prefixes, vec!["app.confirm"]);
    }
}
