use crate::{
    domain::{DomainModel, RowDescriptor},
    filter::ColumnDirective,
    store::{BundleId, KeyId, NodeId},
};


// ── Types ─────────────────────────────────────────────────────────────────────

/// One visual row in the properties table.
///
/// The flat `Vec<ViewRow>` in `AppState` is the single source of truth for
/// both navigation (`cursor_row` indexes into it) and rendering (the renderer
/// paints a window of it).  Built by `enrich_rows` from `DomainModel`
/// descriptors; rebuilt whenever the filter or workspace changes.
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
    /// The store's `KeyId` for the key on this row, or `None` for header rows.
    pub key_id:          Option<KeyId>,
    /// The bundle this row belongs to.  Stable across filter rebuilds.
    pub bundle_id:       BundleId,
    /// The trie `NodeId` this row represents.
    /// - Bundle header → virtual bundle-root node.
    /// - Group header  → deepest node of the chain-collapsed partition group.
    /// - Leaf          → `key_node` of the key.
    /// Used by navigation (equality, ancestry, depth) instead of `Key` strings.
    pub node_id:         NodeId,
    pub is_leaf:         bool,
    pub is_pinned:       bool,
    pub is_temp_pinned:  bool,
    pub is_dangling:     bool,
    pub is_dirty:        bool,
    // ── Cached at build time ──────────────────────────────────────────────────
    bundle_name_s:   String,   // bundle name string (empty for bare keys)
    qualified_str_s: String,   // bundle-qualified row prefix string
    is_bundle_hdr:   bool,     // true for bundle-level header rows
}

impl RowIdentity {
    /// Bundle name this row belongs to; empty string for bare (no-bundle) keys.
    pub fn bundle_name(&self) -> &str { &self.bundle_name_s }

    /// True when this is a bundle-level header row (not a group or leaf row).
    pub fn is_bundle_header(&self) -> bool { self.is_bundle_hdr }

    /// Bundle-qualified prefix string (e.g. `"messages:app.confirm"` or `"messages"`).
    pub fn prefix_str(&self) -> &str { &self.qualified_str_s }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Enrich a list of structural row descriptors into fully-populated view rows.
///
/// Each `RowDescriptor` from `DomainModel::visible_rows` carries structural
/// data only (bundle id, segment ids, key id, indent).  This function resolves
/// segments to strings, looks up translations from the domain model, and attaches
/// dirty/pinned flags to produce `ViewRow`s ready for the renderer.
///
/// All data (translations, locale membership, dangling status) is read from `dm`.
pub fn enrich_rows(
    descriptors: Vec<RowDescriptor>,
    dm: &DomainModel,
    visible_locales: &[String],
    column_directive: ColumnDirective,
) -> Vec<ViewRow> {
    let mut rows = Vec::new();

    for desc in descriptors {
        let bundle_name = dm.bundle_name_str(desc.bundle_id);
        let is_named    = !bundle_name.is_empty();
        // Named bundles: content rows are indented below the bundle header.
        let base_indent = if is_named { 1 } else { 0 };

        // ── Bundle header row ─────────────────────────────────────────────────
        if desc.partitions.is_empty() && desc.key_id.is_none() {
            // Bare-key bundles (empty name) have no visual header row.
            if !is_named { continue; }
            rows.push(ViewRow {
                indent:       0,
                key_segments: vec![bundle_name.to_string()],
                locale_cells: header_cells(visible_locales, bundle_name, dm),
                identity: RowIdentity {
                    key_id:          None,
                    bundle_id:       desc.bundle_id,
                    node_id:         desc.node_id,
                    is_leaf:         false,
                    is_pinned:       false,
                    is_temp_pinned:  false,
                    is_dangling:     false,
                    is_dirty:        false,
                    bundle_name_s:   bundle_name.to_string(),
                    qualified_str_s: dm.node_qualified_str(desc.node_id),
                    is_bundle_hdr:   true,
                },
            });
            continue;
        }

        // Resolve the display segments for this row's key column.
        let key_segments: Vec<String> = desc.partitions.iter()
            .map(|&s| dm.segment_str(s).to_string())
            .collect();

        let indent = base_indent + desc.indent;

        // ── Leaf row ──────────────────────────────────────────────────────────
        if let Some(key_id) = desc.key_id {
            let is_dirty       = dm.is_dirty(key_id);
            let is_pinned      = dm.is_pinned(key_id);
            let is_temp_pinned = dm.is_temp_pinned(key_id);
            let is_dangling    = dm.is_dangling(key_id);

            rows.push(ViewRow {
                indent,
                key_segments,
                locale_cells: leaf_cells(visible_locales, bundle_name, key_id, column_directive, dm),
                identity: RowIdentity {
                    key_id:          Some(key_id),
                    bundle_id:       desc.bundle_id,
                    node_id:         desc.node_id,
                    is_leaf:         true,
                    is_pinned,
                    is_temp_pinned,
                    is_dangling,
                    is_dirty,
                    bundle_name_s:   bundle_name.to_string(),
                    qualified_str_s: dm.node_qualified_str(desc.node_id),
                    is_bundle_hdr:   false,
                },
            });
        } else {
            // ── Group header row ──────────────────────────────────────────────
            rows.push(ViewRow {
                indent,
                key_segments,
                locale_cells: header_cells(visible_locales, bundle_name, dm),
                identity: RowIdentity {
                    key_id:          None,
                    bundle_id:       desc.bundle_id,
                    node_id:         desc.node_id,
                    is_leaf:         false,
                    is_pinned:       false,
                    is_temp_pinned:  false,
                    is_dangling:     false,
                    is_dirty:        false,
                    bundle_name_s:   bundle_name.to_string(),
                    qualified_str_s: dm.node_qualified_str(desc.node_id),
                    is_bundle_hdr:   false,
                },
            });
        }
    }

    rows
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Locale cells for a bundle or group header row.
/// Bundle-owned locales → `Tag`; others → `Empty`.
fn header_cells(
    visible_locales: &[String],
    bundle_name: &str,
    dm: &DomainModel,
) -> Vec<LocaleCellView> {
    visible_locales.iter().map(|locale| {
        let in_bundle = dm.bundle_has_locale(bundle_name, locale);
        let content   = if in_bundle { CellContent::Tag } else { CellContent::Empty };
        LocaleCellView { locale: locale.clone(), content, dirty: false, visible: true }
    }).collect()
}

/// Locale cells for a leaf row, with `visible` flags set by `column_directive`.
///
/// Translation values, locale membership, and per-cell dirty state are all
/// read from `dm` — no external dirty sets needed.
fn leaf_cells(
    visible_locales:  &[String],
    bundle_name:      &str,
    key_id:           KeyId,
    column_directive: ColumnDirective,
    dm:               &DomainModel,
) -> Vec<LocaleCellView> {
    visible_locales.iter().map(|locale| {
        let in_bundle = dm.bundle_has_locale(bundle_name, locale);

        let (content, dirty) = if !in_bundle {
            // Locale has no file in this bundle — Empty, not Missing.
            (CellContent::Empty, false)
        } else {
            let value   = dm.translation_str(key_id, locale);
            let content = match value {
                Some(v) => CellContent::Value(v.to_string()),
                None    => CellContent::Missing,
            };
            let is_dirty = dm.is_dirty_for_locale(key_id, locale);
            (content, is_dirty)
        };

        // Empty cells (locale not in bundle) are never visible on leaf rows.
        let visible = if matches!(content, CellContent::Empty) {
            false
        } else {
            match column_directive {
                ColumnDirective::None        => true,
                ColumnDirective::MissingOnly => matches!(content, CellContent::Missing),
                ColumnDirective::PresentOnly => matches!(content, CellContent::Value(_)),
            }
        };

        LocaleCellView { locale: locale.clone(), content, dirty, visible }
    }).collect()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::DomainModel,
        filter::ColumnDirective,
        store::Store,
    };

    fn sv(strs: &[&str]) -> Vec<String> {
        strs.iter().map(|s| s.to_string()).collect()
    }

    // ── Test helpers ──────────────────────────────────────────────────────────

    /// Build a Store with a named bundle and a set of bare keys (no locales).
    fn store_with_keys(bundle: &str, keys: &[&str]) -> Store {
        let mut store = Store::new();
        for k in keys { store.insert_key(bundle, k); }
        store
    }

    /// Build a Store with locale registration and optional translations.
    ///
    /// `locales` — which locale files the bundle has.
    /// `key_values` — `(locale, key, value)` triples written into the store.
    fn store_for_bundle(bundle: &str, locales: &[&str], key_values: &[(&str, &str, &str)]) -> Store {
        let mut store = Store::new();
        for &l in locales { store.register_locale(bundle, l); }
        for &(locale, key, value) in key_values {
            let kid = store.insert_key(bundle, key);
            store.set_translation(kid, locale, value.to_string());
        }
        store
    }

    fn rows(dm: &DomainModel, visible_locales: &[String]) -> Vec<ViewRow> {
        enrich_rows(dm.visible_rows(|_| true), dm, visible_locales, ColumnDirective::None)
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

    fn prefixes(rows: &[ViewRow]) -> Vec<String> {
        rows.iter().map(|r| r.identity.prefix_str().to_string()).collect()
    }

    // ── Structure ─────────────────────────────────────────────────────────────

    #[test]
    fn named_bundle_header_row() {
        let store = store_with_keys("messages", &["app.title"]);
        let dm    = DomainModel::from_store(store);
        let rows  = rows(&dm, &sv(&["de"]));
        assert_eq!(rows[0].key_segments, vec!["messages"]);
        assert_eq!(rows[0].indent, 0);
        assert!(!rows[0].identity.is_leaf);
        assert!(rows[0].identity.is_bundle_header());
        assert_eq!(rows[0].identity.prefix_str(), "messages");
        assert!(rows[0].identity.key_id.is_none());
    }

    #[test]
    fn named_bundle_content_indented_by_one() {
        let store = store_with_keys("m", &["app.ok", "app.cancel"]);
        let dm    = DomainModel::from_store(store);
        let rows  = rows(&dm, &[]);
        // bundle header (0), app header (1), cancel (2), ok (2)
        assert_eq!(indents(&rows), vec![0, 1, 2, 2]);
        assert_eq!(is_leaf(&rows), vec![false, false, true, true]);
    }

    #[test]
    fn chain_collapsed_row() {
        let store = store_with_keys("m", &["a.b.c"]);
        let dm    = DomainModel::from_store(store);
        let rows  = rows(&dm, &[]);
        assert_eq!(segs(&rows), vec![vec!["m"], vec!["a", "b", "c"]]);
    }

    #[test]
    fn prefix_on_group_header() {
        let store = store_with_keys("msg", &["a.b", "a.c"]);
        let dm    = DomainModel::from_store(store);
        let rows  = rows(&dm, &[]);
        // bundle header, "a" header, "b" leaf, "c" leaf
        assert_eq!(prefixes(&rows), sv(&["msg", "msg:a", "msg:a.b", "msg:a.c"]));
    }

    #[test]
    fn leaf_row_carries_key_id() {
        let store = store_with_keys("m", &["key"]);
        let dm    = DomainModel::from_store(store);
        let rows  = rows(&dm, &[]);
        let leaf  = rows.iter().find(|r| r.identity.is_leaf).unwrap();
        assert!(leaf.identity.key_id.is_some());
    }

    // ── Locale cells ──────────────────────────────────────────────────────────

    #[test]
    fn value_present() {
        let store = store_for_bundle("m", &["de"], &[("de", "key", "Hallo")]);
        let dm    = DomainModel::from_store(store);
        let rows  = rows(&dm, &sv(&["de"]));
        let leaf  = rows.iter().find(|r| r.identity.is_leaf).unwrap();
        assert!(matches!(&leaf.locale_cells[0].content, CellContent::Value(v) if v == "Hallo"));
        assert!(leaf.locale_cells[0].visible);
    }

    #[test]
    fn missing_cell_when_no_translation() {
        // de locale registered, but no translation for "key"
        let store = store_for_bundle("m", &["de"], &[]);
        let mut s = store;
        s.insert_key("m", "key"); // add the key without a translation
        let dm    = DomainModel::from_store(s);
        let rows  = rows(&dm, &sv(&["de"]));
        let leaf  = rows.iter().find(|r| r.identity.is_leaf).unwrap();
        assert!(matches!(leaf.locale_cells[0].content, CellContent::Missing));
        assert!(leaf.locale_cells[0].visible);
    }

    #[test]
    fn empty_cell_when_locale_not_in_bundle() {
        // Bundle only has "de"; visible_locales includes "fr" too.
        let store = store_for_bundle("m", &["de"], &[("de", "key", "Hallo")]);
        let dm    = DomainModel::from_store(store);
        let rows  = rows(&dm, &sv(&["de", "fr"]));
        let leaf  = rows.iter().find(|r| r.identity.is_leaf).unwrap();
        assert!(matches!(leaf.locale_cells[0].content, CellContent::Value(_))); // de
        assert!(matches!(leaf.locale_cells[1].content, CellContent::Empty));    // fr
        assert!(!leaf.locale_cells[1].visible);
    }

    #[test]
    fn header_row_has_tag_for_bundle_locale() {
        // de registered (no translations needed for header test), fr not registered
        let store = store_for_bundle("m", &["de"], &[]);
        let mut s = store;
        s.insert_key("m", "key");
        let dm    = DomainModel::from_store(s);
        let rows  = rows(&dm, &sv(&["de", "fr"]));
        let header = &rows[0]; // bundle header
        assert!(matches!(header.locale_cells[0].content, CellContent::Tag));   // de
        assert!(matches!(header.locale_cells[1].content, CellContent::Empty)); // fr
    }

    #[test]
    fn locale_cell_order_follows_visible_locales() {
        let store = store_for_bundle("m", &["de", "fr"], &[
            ("de", "key", "D"), ("fr", "key", "F"),
        ]);
        let dm   = DomainModel::from_store(store);
        let rows = rows(&dm, &sv(&["fr", "de"])); // fr first
        let leaf = rows.iter().find(|r| r.identity.is_leaf).unwrap();
        assert_eq!(leaf.locale_cells[0].locale, "fr");
        assert_eq!(leaf.locale_cells[1].locale, "de");
    }

    // ── column_directive ──────────────────────────────────────────────────────

    #[test]
    fn missing_only_hides_present_cells() {
        // de has translation, fr registered but missing
        let store = store_for_bundle("m", &["de", "fr"], &[("de", "key", "Hallo")]);
        let mut s = store;
        s.insert_key("m", "key"); // ensure key exists for fr (no translation)
        let dm    = DomainModel::from_store(s);
        let descs = dm.visible_rows(|_| true);
        let rows  = enrich_rows(
            descs, &dm, &sv(&["de", "fr"]),
            ColumnDirective::MissingOnly,
        );
        let leaf = rows.iter().find(|r| r.identity.is_leaf).unwrap();
        assert!(!leaf.locale_cells[0].visible); // de has value → hidden
        assert!(leaf.locale_cells[1].visible);  // fr missing → shown
    }

    #[test]
    fn present_only_hides_missing_cells() {
        // de has translation, fr registered but missing
        let store = store_for_bundle("m", &["de", "fr"], &[("de", "key", "Hallo")]);
        let mut s = store;
        s.insert_key("m", "key");
        let dm    = DomainModel::from_store(s);
        let descs = dm.visible_rows(|_| true);
        let rows  = enrich_rows(
            descs, &dm, &sv(&["de", "fr"]),
            ColumnDirective::PresentOnly,
        );
        let leaf = rows.iter().find(|r| r.identity.is_leaf).unwrap();
        assert!(leaf.locale_cells[0].visible);  // de has value → shown
        assert!(!leaf.locale_cells[1].visible); // fr missing → hidden
    }

    // ── Flags ─────────────────────────────────────────────────────────────────

    #[test]
    fn dirty_flag_on_leaf() {
        // set_translation marks the entry as inserted → key is dirty
        let store = store_for_bundle("m", &["de"], &[("de", "key", "val")]);
        let dm    = DomainModel::from_store(store);
        let descs = dm.visible_rows(|_| true);
        let rows = enrich_rows(
            descs, &dm, &sv(&["de"]),
            ColumnDirective::None,
        );
        let leaf = rows.iter().find(|r| r.identity.is_leaf).unwrap();
        assert!(leaf.identity.is_dirty);
    }

    #[test]
    fn dirty_cell_flag() {
        // store_for_bundle calls set_translation, so the entry is in inserted_entries
        let store = store_for_bundle("m", &["de"], &[("de", "key", "Hallo")]);
        let dm    = DomainModel::from_store(store);
        let descs = dm.visible_rows(|_| true);
        let rows = enrich_rows(
            descs, &dm, &sv(&["de"]),
            ColumnDirective::None,
        );
        let leaf = rows.iter().find(|r| r.identity.is_leaf).unwrap();
        assert!(leaf.locale_cells[0].dirty);
    }
}
