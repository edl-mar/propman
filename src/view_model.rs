use crate::{
    app_model::{common_prefix_len, qualify, DomainModel, Entry},
    filter::ColumnDirective,
};

// ── Types ─────────────────────────────────────────────────────────────────────

/// One visual row in the properties table.
///
/// The flat `Vec<ViewRow>` in `AppState` is the single source of truth for
/// both navigation (`cursor_row` indexes into it) and rendering (the renderer
/// paints a window of it).  Built by `build_view_rows` from the `DomainModel`;
/// rebuilt whenever the filter or workspace changes.
#[derive(Debug)]
pub struct ViewRow {
    /// Visual indentation level; drives `"  ".repeat(indent)` in the renderer.
    pub indent: usize,
    /// The key segments this row displays.
    ///
    /// For chain-collapsed nodes these can span multiple dot-levels,
    /// e.g. `["com", "myapp", "error"]` for a single-child chain that was
    /// compressed in the trie.  `cursor_segment` indexes into this vec.
    pub key_segments: Vec<String>,
    /// One cell per entry in `visible_locales`, in the same order.
    pub locale_cells: Vec<LocaleCellView>,
    /// Identity and metadata used by navigation and editing operations.
    pub identity: RowIdentity,
}

#[derive(Debug)]
pub struct LocaleCellView {
    pub locale:  String,
    pub content: CellContent,
    pub dirty:   bool,
    /// False when `column_directive` hides this cell for this row.
    /// Always `true` for header rows (no value to filter on).
    pub visible: bool,
}

#[derive(Debug)]
pub enum CellContent {
    /// Translation value (leaf rows only).
    Value(String),
    /// Key absent in this locale (leaf rows only).
    Missing,
    /// Locale belongs to this bundle — rendered as `[locale]` tag (header rows).
    Tag,
    /// Locale not in this bundle — cell slot is blank (header rows).
    Empty,
}

#[derive(Debug)]
pub struct RowIdentity {
    pub bundle: String,
    /// Bundle-qualified full key (`"bundle:a.b.c"`), or `None` for header rows.
    pub full_key: Option<String>,
    /// Bundle-qualified prefix this row represents.
    ///
    /// - Bundle header: just the bundle name (`"messages"`).
    /// - Group header:  `"messages:app.confirm"`.
    /// - Leaf:          same as `full_key`.
    ///
    /// Used by Left navigation (find ancestor row) and scope highlighting
    /// (descendant rows share this prefix).
    pub prefix:         String,
    pub is_leaf:        bool,
    pub is_pinned:      bool,
    pub is_temp_pinned: bool,
    pub is_dangling:    bool,
    pub is_dirty:       bool,
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Build the flat view row list from the current domain model.
///
/// Emits rows in display order:
/// - One bundle header row per named bundle.
/// - For each entry: zero or more group header rows (newly introduced render
///   boundaries, using trie + consecutive comparison) followed by one leaf row.
///
/// The `column_directive` sets the `visible` flag on leaf locale cells; header
/// locale cells are always visible.
pub fn build_view_rows(
    dm: &DomainModel,
    visible_locales: &[String],
    column_directive: ColumnDirective,
) -> Vec<ViewRow> {
    let mut rows = Vec::new();

    for bundle in &dm.bundles {
        let is_named    = !bundle.name.is_empty();
        // Content inside a named bundle is indented by 1 (below the header).
        let base_indent = if is_named { 1 } else { 0 };

        // ── Bundle header row ─────────────────────────────────────────────────
        if is_named {
            rows.push(ViewRow {
                indent:       0,
                key_segments: vec![bundle.name.clone()],
                locale_cells: header_cells(visible_locales, &bundle.locales),
                identity: RowIdentity {
                    bundle:         bundle.name.clone(),
                    full_key:       None,
                    prefix:         bundle.name.clone(),
                    is_leaf:        false,
                    is_pinned:      false,
                    is_temp_pinned: false,
                    is_dangling:    false,
                    is_dirty:       false,
                },
            });
        }

        let mut prev_segs: &[String] = &[];

        for entry in &bundle.entries {
            let segs     = &entry.segments;
            let shared   = common_prefix_len(segs, prev_segs);
            let full_key = qualify(&bundle.name, &entry.real_key());

            let seg_strs: Vec<&str> = segs.iter().map(|s| s.as_str()).collect();
            let partitions = bundle.trie.key_partitions(&seg_strs);
            // The last partition is the leaf; all earlier ones are group headers.
            let (leaf_range, header_ranges) = partitions.split_last()
                .expect("key_partitions always returns at least one range");

            for (gi, range) in header_ranges.iter().enumerate() {
                // Skip headers shared with the previous entry (already emitted).
                // A partition ending at or before `shared` is fully within the
                // common prefix — its header row was already rendered.
                if range.end <= shared { continue; }
                let prefix = qualify_prefix(&bundle.name, segs, range.end - 1);
                rows.push(ViewRow {
                    indent:       base_indent + gi,
                    key_segments: segs[range.clone()].to_vec(),
                    locale_cells: header_cells(visible_locales, &bundle.locales),
                    identity: RowIdentity {
                        bundle:         bundle.name.clone(),
                        full_key:       None,
                        prefix,
                        is_leaf:        false,
                        is_pinned:      false,
                        is_temp_pinned: false,
                        is_dangling:    false,
                        is_dirty:       false,
                    },
                });
            }

            // ── Leaf row ──────────────────────────────────────────────────────
            let gi           = header_ranges.len();
            let locale_cells = leaf_cells(visible_locales, &bundle.locales, entry, &column_directive);
            rows.push(ViewRow {
                indent:       base_indent + gi,
                key_segments: segs[leaf_range.clone()].to_vec(),
                locale_cells,
                identity: RowIdentity {
                    bundle:         bundle.name.clone(),
                    full_key:       Some(full_key.clone()),
                    prefix:         full_key,
                    is_leaf:        true,
                    is_pinned:      entry.is_pinned,
                    is_temp_pinned: entry.is_temp_pinned,
                    is_dangling:    entry.is_dangling,
                    is_dirty:       entry.is_dirty,
                },
            });

            prev_segs = segs;
        }
    }

    rows
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Locale cells for a bundle or group header row.
/// Bundle-owned locales → `Tag`; others → `Empty`.
fn header_cells(visible_locales: &[String], bundle_locales: &[String]) -> Vec<LocaleCellView> {
    visible_locales.iter().map(|locale| {
        let content = if bundle_locales.contains(locale) {
            CellContent::Tag
        } else {
            CellContent::Empty
        };
        LocaleCellView { locale: locale.clone(), content, dirty: false, visible: true }
    }).collect()
}

/// Locale cells for a leaf row, with `visible` flags set by `column_directive`.
fn leaf_cells(
    visible_locales:  &[String],
    bundle_locales:   &[String],
    entry:            &Entry,
    column_directive: &ColumnDirective,
) -> Vec<LocaleCellView> {
    visible_locales.iter().map(|locale| {
        let cell_opt = bundle_locales.iter()
            .position(|l| l == locale)
            .and_then(|i| entry.cells.get(i));

        let (content, dirty) = match cell_opt {
            Some(cell) => {
                let c = match &cell.value {
                    Some(v) => CellContent::Value(v.clone()),
                    None    => CellContent::Missing,
                };
                (c, cell.is_dirty)
            }
            // Locale has no file in this bundle — treat as Empty, not Missing.
            // Missing means "the bundle has a locale file but this key is absent";
            // Empty means "the bundle doesn't support this locale at all".
            None => (CellContent::Empty, false),
        };

        // Empty cells (locale not in this bundle) are never shown on leaf rows.
        let visible = if matches!(content, CellContent::Empty) {
            false
        } else {
            match *column_directive {
                ColumnDirective::None        => true,
                ColumnDirective::MissingOnly => matches!(content, CellContent::Missing),
                ColumnDirective::PresentOnly => matches!(content, CellContent::Value(_)),
            }
        };

        LocaleCellView { locale: locale.clone(), content, dirty, visible }
    }).collect()
}

/// Build the bundle-qualified prefix string for a group header at depth `d`.
/// `segs[..=d]` is the full path; `bundle` is empty for bare-key bundles.
fn qualify_prefix(bundle: &str, segs: &[String], d: usize) -> String {
    let key_part = segs[..=d].join(".");
    qualify(bundle, &key_part)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app_model::{BundleModel, DomainModel, Entry, LocaleCell},
        filter::ColumnDirective,
        radix_tree_arena::CompressedTrie,
    };

    // ── Test helpers ──────────────────────────────────────────────────────────

    fn sv(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    fn bare_model(keys: &[&str]) -> DomainModel {
        named_model("", keys, &[])
    }

    fn named_model(bundle: &str, keys: &[&str], locales: &[&str]) -> DomainModel {
        let mut sorted: Vec<String> = keys.iter().map(|s| s.to_string()).collect();
        sorted.sort();

        let bundle_locales: Vec<String> = locales.iter().map(|s| s.to_string()).collect();

        let entries: Vec<Entry> = sorted.iter().map(|k| {
            let cells: Vec<LocaleCell> = bundle_locales.iter()
                .map(|_| LocaleCell { value: None, is_dirty: false })
                .collect();
            Entry {
                segments: k.split('.').map(|s| s.to_string()).collect(),
                cells,
                is_dirty: false, is_pinned: false,
                is_temp_pinned: false, is_dangling: false,
            }
        }).collect();

        let mut trie = CompressedTrie::new();
        for (i, e) in entries.iter().enumerate() {
            trie.insert_str(&e.real_key(), i);
        }

        DomainModel {
            bundles: vec![BundleModel {
                name: bundle.to_string(),
                locales: bundle_locales,
                entries,
                trie,
            }],
        }
    }

    fn segs(rows: &[ViewRow]) -> Vec<Vec<&str>> {
        rows.iter()
            .map(|r| r.key_segments.iter().map(|s| s.as_str()).collect())
            .collect()
    }

    fn indents(rows: &[ViewRow]) -> Vec<usize> {
        rows.iter().map(|r| r.indent).collect()
    }

    fn is_leaf(rows: &[ViewRow]) -> Vec<bool> {
        rows.iter().map(|r| r.identity.is_leaf).collect()
    }

    fn prefixes(rows: &[ViewRow]) -> Vec<&str> {
        rows.iter().map(|r| r.identity.prefix.as_str()).collect()
    }

    fn full_keys(rows: &[ViewRow]) -> Vec<Option<&str>> {
        rows.iter().map(|r| r.identity.full_key.as_deref()).collect()
    }

    // ── Row structure tests (bare keys, no bundle) ────────────────────────────

    #[test]
    fn siblings_get_a_header() {
        let dm   = bare_model(&["app.confirm.delete", "app.confirm.discard"]);
        let rows = build_view_rows(&dm, &[], ColumnDirective::None);
        assert_eq!(segs(&rows),    vec![vec!["app","confirm"], vec!["delete"], vec!["discard"]]);
        assert_eq!(indents(&rows), vec![0, 1, 1]);
        assert_eq!(is_leaf(&rows), vec![false, true, true]);
        assert_eq!(prefixes(&rows), vec!["app.confirm", "app.confirm.delete", "app.confirm.discard"]);
    }

    #[test]
    fn lone_key_chain_collapses() {
        let dm   = bare_model(&["app.loading"]);
        let rows = build_view_rows(&dm, &[], ColumnDirective::None);
        // Single entry — no branch node anywhere — emits one leaf at indent 0.
        assert_eq!(rows.len(), 1);
        assert_eq!(segs(&rows),    vec![vec!["app", "loading"]]);
        assert_eq!(indents(&rows), vec![0]);
        assert!(rows[0].identity.is_leaf);
    }

    #[test]
    fn truly_bare_key() {
        let dm   = bare_model(&["loading"]);
        let rows = build_view_rows(&dm, &[], ColumnDirective::None);
        assert_eq!(rows.len(), 1);
        assert_eq!(segs(&rows),    vec![vec!["loading"]]);
        assert_eq!(indents(&rows), vec![0]);
        assert!(rows[0].identity.is_leaf);
    }

    #[test]
    fn single_child_non_key_chain_collapses() {
        // com→myapp→error is a single-child chain — no branch — collapses to one header.
        let dm   = bare_model(&["com.myapp.error.notfound", "com.myapp.error.timeout"]);
        let rows = build_view_rows(&dm, &[], ColumnDirective::None);
        // Header spans three segments: ["com","myapp","error"]
        assert_eq!(segs(&rows)[0],    vec!["com", "myapp", "error"]);
        assert_eq!(indents(&rows),    vec![0, 1, 1]);
        assert_eq!(is_leaf(&rows),    vec![false, true, true]);
        assert_eq!(prefixes(&rows),   vec!["com.myapp.error", "com.myapp.error.notfound", "com.myapp.error.timeout"]);
    }

    #[test]
    fn mixed_depth_siblings() {
        let dm   = bare_model(&["app.confirm.delete", "app.confirm.discard", "app.loading"]);
        let rows = build_view_rows(&dm, &[], ColumnDirective::None);
        assert_eq!(segs(&rows), vec![
            vec!["app"],
            vec!["confirm"],
            vec!["delete"],
            vec!["discard"],
            vec!["loading"],
        ]);
        assert_eq!(indents(&rows), vec![0, 1, 2, 2, 1]);
        assert_eq!(is_leaf(&rows), vec![false, false, true, true, true]);
    }

    #[test]
    fn key_that_is_also_a_parent_emits_no_header() {
        // app.x is a key (Interior) AND parent of a, b — Interior never gets a
        // header row for itself; children appear one level deeper.
        let dm   = bare_model(&["app.x", "app.x.a", "app.x.b"]);
        let rows = build_view_rows(&dm, &[], ColumnDirective::None);
        assert_eq!(segs(&rows), vec![vec!["app","x"], vec!["a"], vec!["b"]]);
        assert_eq!(indents(&rows), vec![0, 1, 1]);
        assert_eq!(is_leaf(&rows), vec![true, true, true]);
        // No group header rows.
        assert!(!rows.iter().any(|r| r.identity.full_key.is_none()));
    }

    #[test]
    fn nested_headers() {
        let dm   = bare_model(&["app.a.b.c", "app.a.b.d", "app.a.e", "app.a.f"]);
        let rows = build_view_rows(&dm, &[], ColumnDirective::None);
        assert_eq!(segs(&rows), vec![
            vec!["app","a"],
            vec!["b"],
            vec!["c"],
            vec!["d"],
            vec!["e"],
            vec!["f"],
        ]);
        assert_eq!(indents(&rows), vec![0, 1, 2, 2, 1, 1]);
        assert_eq!(is_leaf(&rows), vec![false, false, true, true, true, true]);
    }

    // ── Named bundle ──────────────────────────────────────────────────────────

    #[test]
    fn named_bundle_gets_header_row() {
        let dm   = named_model("messages", &["app.title", "app.subtitle"], &["de", "fr"]);
        let rows = build_view_rows(&dm, &sv(&["de", "fr"]), ColumnDirective::None);
        // Row 0: bundle header.
        assert_eq!(rows[0].key_segments, vec!["messages"]);
        assert_eq!(rows[0].indent, 0);
        assert!(!rows[0].identity.is_leaf);
        assert_eq!(rows[0].identity.prefix, "messages");
        assert_eq!(rows[0].identity.full_key, None);
        // Bundle header locale cells are Tags.
        assert!(rows[0].locale_cells.iter().all(|c| matches!(c.content, CellContent::Tag)));
        // Content rows are shifted by 1.
        // app.title and app.subtitle share no branch → chain-collapse into one header "app".
        // Wait — "app" has 2 children (title, subtitle) → Branch → header.
        assert_eq!(rows[1].key_segments, vec!["app"]);
        assert_eq!(rows[1].indent, 1);  // base_indent=1, gi=0 → 1
        assert!(!rows[1].identity.is_leaf);
        assert_eq!(rows[2].key_segments, vec!["subtitle"]);
        assert_eq!(rows[2].indent, 2);
        assert!(rows[2].identity.is_leaf);
        assert_eq!(rows[3].key_segments, vec!["title"]);
        assert_eq!(rows[3].indent, 2);
        assert!(rows[3].identity.is_leaf);
    }

    #[test]
    fn named_bundle_prefixes_are_bundle_qualified() {
        let dm   = named_model("msg", &["a.b", "a.c"], &[]);
        let rows = build_view_rows(&dm, &[], ColumnDirective::None);
        // bundle header, group header "a", leaf "b", leaf "c"
        assert_eq!(prefixes(&rows), vec!["msg", "msg:a", "msg:a.b", "msg:a.c"]);
        assert_eq!(full_keys(&rows), vec![None, None, Some("msg:a.b"), Some("msg:a.c")]);
    }

    // ── Locale cells ──────────────────────────────────────────────────────────

    #[test]
    fn leaf_locale_cells_match_visible_locales_order() {
        let dm   = named_model("m", &["key"], &["de", "fr"]);
        let rows = build_view_rows(&dm, &sv(&["fr", "de"]), ColumnDirective::None);
        // Leaf row is the last row.
        let leaf = rows.iter().last().unwrap();
        assert_eq!(leaf.locale_cells[0].locale, "fr");
        assert_eq!(leaf.locale_cells[1].locale, "de");
    }

    #[test]
    fn locale_not_in_bundle_is_empty_on_header() {
        // visible_locales includes "it" but bundle only has "de".
        let dm   = named_model("m", &["key"], &["de"]);
        let rows = build_view_rows(&dm, &sv(&["de", "it"]), ColumnDirective::None);
        let header = &rows[0]; // bundle header
        assert!(matches!(header.locale_cells[0].content, CellContent::Tag));   // de → Tag
        assert!(matches!(header.locale_cells[1].content, CellContent::Empty)); // it → Empty
    }

    #[test]
    fn column_directive_missing_only_hides_present_cells() {
        // Build a model where one locale has a value and another doesn't.
        // We do this by constructing Entry.cells manually.
        let mut dm = named_model("m", &["key"], &["de", "fr"]);
        // Set de=Some("Hallo"), fr=None.
        dm.bundles[0].entries[0].cells[0].value = Some("Hallo".to_string());
        // fr stays None (Missing).

        let rows = build_view_rows(&dm, &sv(&["de", "fr"]), ColumnDirective::MissingOnly);
        let leaf = rows.iter().last().unwrap();
        // de has a value → not visible under MissingOnly.
        assert!(!leaf.locale_cells[0].visible); // de
        // fr is Missing → visible.
        assert!(leaf.locale_cells[1].visible);  // fr
    }

    #[test]
    fn column_directive_present_only_hides_missing_cells() {
        let mut dm = named_model("m", &["key"], &["de", "fr"]);
        dm.bundles[0].entries[0].cells[0].value = Some("Hallo".to_string());

        let rows = build_view_rows(&dm, &sv(&["de", "fr"]), ColumnDirective::PresentOnly);
        let leaf = rows.iter().last().unwrap();
        assert!(leaf.locale_cells[0].visible);  // de has value → visible
        assert!(!leaf.locale_cells[1].visible); // fr Missing → not visible
    }
}
