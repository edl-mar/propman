use std::collections::HashSet;
use crate::store::{BundleId, Change, EntryId, GlobalSegmentId, KeyId, NodeId, Store};

// ── App Model (domain model) ──────────────────────────────────────────────────

/// The complete hierarchical app model.  Single source of truth for navigation,
/// filter evaluation, and rendering.
#[derive(Debug, Default)]
pub struct DomainModel {
    /// Central data store: partition graph, translations, and exception tables.
    /// Long-lived — never rebuilt after startup; structural queries run against it.
    /// Private: all access goes through `DomainModel` methods.
    store: Store,
}

impl DomainModel {
    /// Build the initial `DomainModel` from the workspace.
    pub fn from_workspace(ws: &crate::workspace::Workspace) -> Self {
        Self {
            store: Store::from_workspace(ws),
        }
    }

    /// Construct a `DomainModel` from a pre-built store.
    /// Used by tests in `domain` and `view_model` that build a store directly.
    #[cfg(test)]
    pub(crate) fn from_store(store: Store) -> Self {
        Self { store }
    }
}

// ── Store delegation — view-layer access ─────────────────────────────────────
//
// The store is private; the view layer (view_model.rs) accesses store data
// exclusively through these methods.  All string resolution and translation
// lookups for enriching RowDescriptors into ViewRows go through here.

impl DomainModel {
    pub fn bundle_name_str(&self, bundle_id: BundleId) -> &str {
        self.store.bundle_name_str(bundle_id)
    }

    pub fn segment_str(&self, seg: GlobalSegmentId) -> &str {
        self.store.segment_str(seg)
    }

    pub fn key_qualified_str(&self, key_id: KeyId) -> String {
        self.store.key_qualified_str(key_id)
    }

    /// The name of the bundle that owns `key_id`.
    pub fn bundle_name_for_key(&self, key_id: KeyId) -> &str {
        self.store.bundle_name_str(self.store.key_bundle(key_id))
    }

    /// The bare dot-joined real key string for `key_id` (no bundle prefix).
    pub fn real_key_str(&self, key_id: KeyId) -> String {
        self.store.key_real_key_str(key_id)
    }

    /// Locale strings registered for `bundle`, in registration order.
    /// For bare (empty) bundle names, returns all locale strings across all bundles.
    pub fn bundle_locale_strings(&self, bundle: &str) -> Vec<String> {
        if bundle.is_empty() {
            return self.all_locale_strings();
        }
        match self.store.find_bundle(bundle) {
            None => vec![],
            Some(bid) => self
                .store
                .bundle_locales(bid)
                .iter()
                .map(|&lid| self.store.locale_str(lid).to_string())
                .collect(),
        }
    }

    /// Returns `true` when `bundle` has a locale file for `locale`.
    /// Always returns `true` for bare (empty) bundle names.
    pub fn bundle_has_locale(&self, bundle: &str, locale: &str) -> bool {
        if bundle.is_empty() {
            return true;
        }
        match self.store.find_bundle(bundle) {
            None => false,
            Some(bid) => self.store.bundle_has_locale_str(bid, locale),
        }
    }

    /// Look up a translation for `key_id` in `locale`.
    /// Returns `None` when the key has no translation for this locale.
    pub fn translation_str(&self, key_id: KeyId, locale: &str) -> Option<&str> {
        self.store.translation_for_str(key_id, locale)
    }

    /// Returns `true` when `key_id` has no translations in any of its bundle's locales.
    /// A key is dangling when it was created this session but no value has been written.
    pub fn is_dangling(&self, key_id: KeyId) -> bool {
        let bundle_id = self.store.key_bundle(key_id);
        self.store
            .bundle_locales(bundle_id)
            .iter()
            .all(|&lid| self.store.translation(key_id, lid).is_none())
    }

    /// Returns `true` when `name` is a known bundle name.
    /// Replaces `workspace.is_bundle_name` for cross-bundle rename guards.
    pub fn is_bundle_name(&self, name: &str) -> bool {
        self.store.find_bundle(name).is_some()
    }

    /// All distinct locale strings across all bundles, in first-registration order.
    /// Replaces `workspace.all_locales()` for locale-column setup.
    pub fn all_locale_strings(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for bid in self.store.bundle_ids() {
            for &lid in self.store.bundle_locales(bid) {
                let s = self.store.locale_str(lid).to_string();
                if seen.insert(s.clone()) {
                    out.push(s);
                }
            }
        }
        out
    }
}

// ── Store delegation — mutation layer ────────────────────────────────────────
//
// Ops call these to keep the store in sync with workspace mutations.
// Each method is a thin delegate into the private store.

impl DomainModel {
    /// Look up a key by its bundle-qualified name.
    /// Returns `None` when the key is not in the store (never inserted or deleted).
    pub fn find_key(&self, full_key: &str) -> Option<KeyId> {
        let (bundle, real_key) = split_key(full_key);
        self.store.find_key(bundle, real_key)
    }

    /// Permanently remove a key from the store.
    /// See `Store::delete_key` for the full contract (trie pruning, etc.).
    pub fn delete_key(&mut self, key_id: KeyId) {
        self.store.delete_key(key_id);
    }

    /// Register `(bundle, real_key)` in the store, returning its `KeyId`.
    /// Idempotent: returns the existing id when called twice for the same key.
    pub fn insert_key(&mut self, bundle: &str, real_key: &str) -> KeyId {
        self.store.insert_key(bundle, real_key)
    }

    /// Register a locale file for a bundle (idempotent).
    pub fn register_locale(&mut self, bundle: &str, locale: &str) {
        self.store.register_locale(bundle, locale);
    }

    /// Store or update a translation (user mutation — records a change).
    pub fn set_translation(&mut self, key_id: KeyId, locale: &str, value: String) {
        self.store.set_translation(key_id, locale, value);
    }

    /// Remove a translation (locale cell deleted by the user).
    pub fn remove_translation(&mut self, key_id: KeyId, locale: &str) {
        self.store.remove_translation(key_id, locale);
    }

    // ── Change-set ────────────────────────────────────────────────────────────

    /// `true` when there are any unsaved changes.
    pub fn has_changes(&self) -> bool {
        self.store.has_changes()
    }

    /// `true` when `key_id` has any unsaved changes.
    pub fn is_dirty(&self, key_id: KeyId) -> bool {
        self.store.is_dirty(key_id)
    }

    /// `true` when the specific `(key_id, locale)` cell has unsaved changes.
    pub fn is_dirty_for_locale(&self, key_id: KeyId, locale: &str) -> bool {
        self.store.is_dirty_for_locale(key_id, locale)
    }

    /// Locale strings that have at least one pending change.
    /// Used by `apply_filter` to derive dirty locale columns for `:#`/`#` filter terms.
    pub fn dirty_locale_strings(&self) -> HashSet<String> {
        let mut out = HashSet::new();
        for change in self.store.change_set() {
            let eid = match change {
                Change::Insert { entry_id } | Change::Update { entry_id } | Change::Delete { entry_id } => entry_id,
            };
            out.insert(self.store.locale_str(self.store.entry_locale_id(eid)).to_string());
        }
        out
    }

    /// All pending changes; consumed by `workspace::save`.
    pub fn change_set(&self) -> impl Iterator<Item = Change> + '_ {
        self.store.change_set()
    }

    /// Reset the change log after a successful save.
    pub fn clear_changes(&mut self) {
        self.store.clear_changes();
    }

    // ── Entry accessors (for workspace::save) ─────────────────────────────────

    pub fn entry_key_id(&self, eid: EntryId) -> KeyId {
        self.store.entry_key_id(eid)
    }

    pub fn entry_locale_str(&self, eid: EntryId) -> &str {
        self.store.locale_str(self.store.entry_locale_id(eid))
    }

    pub fn entry_bundle_name(&self, eid: EntryId) -> &str {
        self.bundle_name_for_key(self.store.entry_key_id(eid))
    }

    pub fn entry_real_key(&self, eid: EntryId) -> String {
        self.real_key_str(self.store.entry_key_id(eid))
    }

    pub fn entry_current_value(&self, eid: EntryId) -> Option<&str> {
        self.store.entry_current_value(eid)
    }

    // ── Pinning ───────────────────────────────────────────────────────────────

    pub fn is_pinned(&self, key_id: KeyId) -> bool {
        self.store.is_pinned(key_id)
    }
    pub fn pin_key(&mut self, key_id: KeyId) {
        self.store.pin_key(key_id);
    }
    pub fn unpin_key(&mut self, key_id: KeyId) {
        self.store.unpin_key(key_id);
    }

    // ── Temp-pins ─────────────────────────────────────────────────────────────

    pub fn is_temp_pinned(&self, key_id: KeyId) -> bool {
        self.store.is_temp_pinned(key_id)
    }
    pub fn has_temp_pins(&self) -> bool {
        self.store.has_temp_pins()
    }
    pub fn set_temp_pins(&mut self, pins: Vec<KeyId>) {
        self.store.set_temp_pins(pins);
    }
    pub fn clear_temp_pins(&mut self) {
        self.store.clear_temp_pins();
    }
    pub fn temp_pins_match(&self, candidate: &[KeyId]) -> bool {
        self.store.temp_pins_match(candidate)
    }

    /// All non-deleted key IDs across all bundles, in insertion order.
    pub fn all_key_ids(&self) -> impl Iterator<Item = KeyId> + '_ {
        self.store.all_key_ids()
    }

    // ── Node navigation ───────────────────────────────────────────────────────

    /// Number of segments from the bundle root to `nid` (root node = depth 0).
    pub fn node_depth(&self, nid: NodeId) -> usize { self.store.node_depth(nid) }

    /// Direct parent of `nid`, or `None` when `nid` is the bundle root.
    pub fn node_parent_id(&self, nid: NodeId) -> Option<NodeId> { self.store.node_parent(nid) }

    /// Ancestor of `nid` at exactly `depth` segments from the bundle root.
    pub fn node_ancestor_at_depth(&self, nid: NodeId, depth: usize) -> NodeId {
        self.store.node_ancestor_at_depth(nid, depth)
    }

    /// `true` when `ancestor` is a strict (not equal) ancestor of `nid`.
    pub fn node_is_strict_ancestor(&self, ancestor: NodeId, nid: NodeId) -> bool {
        self.store.node_is_strict_ancestor(ancestor, nid)
    }

    /// `BundleId` of the bundle that owns `nid`.
    pub fn node_bundle_id(&self, nid: NodeId) -> BundleId { self.store.node_bundle(nid) }

    /// The trie `NodeId` for the leaf node that belongs to `key_id`.
    pub fn key_node_id(&self, key_id: KeyId) -> NodeId { self.store.key_node(key_id) }

    /// Bundle-qualified string for trie node `nid`.
    pub fn node_qualified_str(&self, nid: NodeId) -> String { self.store.node_qualified_str(nid) }

    /// Copy all translations from `src_key_id` to a new key at `(dest_bundle, dest_real)`.
    ///
    /// For each locale in the source key's bundle: if `dest_bundle` also has that
    /// locale, the translation is written to the destination key (which is created
    /// idempotently).  Locales that exist in the source bundle but not in
    /// `dest_bundle` are returned as missed locale name strings.
    ///
    /// Source key is left untouched — callers that want a move must call
    /// `delete_key(src_key_id)` after this returns.
    pub fn copy_translations_to(
        &mut self,
        src_key_id: KeyId,
        dest_bundle: &str,
        dest_real: &str,
    ) -> Vec<String> {
        let src_bundle_id = self.store.key_bundle(src_key_id);
        let locale_ids: Vec<_> = self.store.bundle_locales(src_bundle_id).iter().copied().collect();

        let pairs: Vec<(String, String)> = locale_ids.iter()
            .filter_map(|&lid| {
                self.store.translation(src_key_id, lid)
                    .map(|v| (self.store.locale_str(lid).to_string(), v.to_string()))
            })
            .collect();

        let mut missed = Vec::new();
        for (locale, value) in pairs {
            if self.bundle_has_locale(dest_bundle, &locale) {
                let dest_key_id = self.store.insert_key(dest_bundle, dest_real);
                self.store.set_translation(dest_key_id, &locale, value);
            } else {
                missed.push(locale);
            }
        }
        missed
    }

    /// Rename a key by creating it at `new_full_key`, copying all translations
    /// from `old_key_id`, and then deleting the old key.
    ///
    /// Returns the new `KeyId`.  Works for both same-bundle and cross-bundle
    /// renames — the new bundle must already exist in the store (locale files
    /// registered) for its keys to be renderable.
    pub fn rename_key(&mut self, old_key_id: KeyId, new_full_key: &str) -> KeyId {
        let (new_bundle, new_real) = split_key(new_full_key);

        // Create (or find) the node at the new path.
        let new_key_id = self.store.insert_key(new_bundle, new_real);

        // Copy all translations from the old key to the new one.
        // We collect locale ids first to avoid borrow conflicts.
        let locale_ids: Vec<_> = self
            .store
            .bundle_locales(self.store.key_bundle(old_key_id))
            .iter()
            .copied()
            .collect();
        for lid in locale_ids {
            if let Some(val) = self.store.translation(old_key_id, lid) {
                let val = val.to_string();
                // Use the locale string for `set_translation`'s intern lookup.
                let locale_str = self.store.locale_str(lid).to_string();
                self.store.set_translation(new_key_id, &locale_str, val);
            }
        }

        // Soft-delete (and prune) the old key.
        self.store.delete_key(old_key_id);

        new_key_id
    }
}

// ── Row descriptor ────────────────────────────────────────────────────────────

/// A single visible table row, returned by `DomainModel::visible_rows`.
///
/// Carries structural information only.  Locale cell values are looked up
/// separately by the view layer via the store.
#[derive(Debug, Clone)]
pub struct RowDescriptor {
    pub bundle_id: BundleId,
    /// The trie node this row represents.
    /// - Bundle header → virtual bundle-root node.
    /// - Group header  → last node of the chain-collapsed partition group.
    /// - Leaf          → `key_node` of the key.
    pub node_id: NodeId,
    /// Segments to display for this row, in root-to-leaf order.
    /// These are the chain-collapsed segments since the last visible branch —
    /// i.e., what belongs on this visual row.  Empty for bundle-header rows.
    pub partitions: Vec<GlobalSegmentId>,
    /// Full segment path from the bundle root to the deepest segment of this
    /// row, in root-to-leaf order.  Empty for bundle-header rows.
    ///
    /// Concretely: all segments from `groups[0]` through `groups[indent]`
    /// (the partition groups up to and including this row's group) concatenated.
    /// The view layer uses this to reconstruct the `Key::prefix` for navigation
    /// without needing to re-run `key_compressed`.
    pub prefix_segs: Vec<GlobalSegmentId>,
    /// `Some` for leaf rows (translatable keys), `None` for group/bundle headers.
    pub key_id: Option<KeyId>,
    /// Visual indentation level within the bundle (0 = top-level group/leaf row).
    /// Named bundles add a +1 base indent in the view layer (below the header).
    pub indent: usize,
}

impl DomainModel {
    /// Return an ordered sequence of row descriptors matching the current filter.
    ///
    /// `key_visible(key_id)` is called for every leaf key.  When it returns
    /// `false` the key row is omitted; group headers are suppressed automatically
    /// (a header is only emitted when a visible key actually falls under it).
    /// Bundle headers are always emitted.
    ///
    /// Algorithm: process keys in sorted order, derive group headers from
    /// `key_compressed` by emitting a new header whenever a key introduces a
    /// partition group not seen in the previous key.
    pub fn visible_rows(&self, key_visible: impl Fn(KeyId) -> bool) -> Vec<RowDescriptor> {
        let store = &self.store;
        let mut rows = Vec::new();

        // Stable bundle order: alphabetical by name.
        let mut bundle_ids: Vec<BundleId> = store.bundle_ids().collect();
        bundle_ids.sort_by_key(|&b| store.bundle_name_str(b));

        for bundle_id in bundle_ids {
            // Bundle-header row — always emitted.
            rows.push(RowDescriptor {
                bundle_id,
                node_id:     store.bundle_root(bundle_id),
                partitions:  vec![],
                prefix_segs: vec![],
                key_id:      None,
                indent:      0,
            });

            // Collect visible keys for this bundle, sorted by their dot-joined path.
            let mut visible_keys: Vec<KeyId> = store
                .bundle_keys(bundle_id)
                .filter(|&k| key_visible(k))
                .collect();
            visible_keys.sort_by_key(|&k| store.key_real_key_str(k));

            // Sweep through sorted keys: emit group headers once per new partition
            // prefix, then the leaf row.  `prev_groups` tracks what the previous key
            // emitted so we know which headers are already on screen.
            let mut prev_groups: Vec<Vec<GlobalSegmentId>> = vec![];
            for key_id in visible_keys {
                let groups    = store.key_compressed(key_id);
                let shared    = groups_shared_prefix(&prev_groups, &groups);
                let leaf_gi   = groups.len() - 1;
                let leaf_node = store.key_node(key_id);

                // Emit a group header for every new partition group except the last
                // (the last group is the leaf row itself).
                for gi in shared..leaf_gi {
                    let depth_at_gi: usize = groups[..=gi].iter().map(|g| g.len()).sum();
                    let group_node  = store.node_ancestor_at_depth(leaf_node, depth_at_gi);
                    let prefix_segs = groups[..=gi]
                        .iter()
                        .flat_map(|g| g.iter().copied())
                        .collect();
                    rows.push(RowDescriptor {
                        bundle_id,
                        node_id:     group_node,
                        partitions:  groups[gi].clone(),
                        prefix_segs,
                        key_id:      None,
                        indent:      gi,
                    });
                }

                // Emit the leaf row.
                let prefix_segs = groups.iter().flat_map(|g| g.iter().copied()).collect();
                rows.push(RowDescriptor {
                    bundle_id,
                    node_id:     leaf_node,
                    partitions:  groups[leaf_gi].clone(),
                    prefix_segs,
                    key_id:      Some(key_id),
                    indent:      leaf_gi,
                });

                prev_groups = groups;
            }
        }

        rows
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Number of leading groups shared between `a` and `b`.
fn groups_shared_prefix(a: &[Vec<GlobalSegmentId>], b: &[Vec<GlobalSegmentId>]) -> usize {
    a.iter().zip(b.iter()).take_while(|(p, c)| p == c).count()
}

/// Splits a bundle-qualified key string into `(bundle, real_key)`.
///
/// `"messages:app.title"` → `("messages", "app.title")`
/// `"app.title"`          → `("", "app.title")` (no bundle prefix)
///
/// This is the single canonical parse point for the `bundle:key` boundary.
/// All callers — ops, update — must use this function rather than calling
/// `workspace::split_key` directly.
pub fn split_key(full_key: &str) -> (&str, &str) {
    match full_key.find(':') {
        Some(idx) => (&full_key[..idx], &full_key[idx + 1..]),
        None      => ("", full_key),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── visible_rows tests ────────────────────────────────────────────────────
    //
    // Build a Store directly, wrap in DomainModel, call visible_rows.

    fn sv(s: &[&str]) -> Vec<String> {
        s.iter().map(|s| s.to_string()).collect()
    }

    fn store_with_keys(bundle: &str, keys: &[&str]) -> DomainModel {
        let mut store = Store::new();
        for k in keys {
            store.insert_key(bundle, k);
        }
        DomainModel::from_store(store)
    }

    fn row_segs(dm: &DomainModel) -> Vec<Vec<String>> {
        dm.visible_rows(|_| true)
            .into_iter()
            .map(|r| {
                r.partitions
                    .iter()
                    .map(|&s| dm.store.segment_str(s).to_string())
                    .collect()
            })
            .collect()
    }

    fn row_indents(dm: &DomainModel) -> Vec<usize> {
        dm.visible_rows(|_| true).iter().map(|r| r.indent).collect()
    }

    fn row_is_leaf(dm: &DomainModel) -> Vec<bool> {
        dm.visible_rows(|_| true)
            .iter()
            .map(|r| r.key_id.is_some())
            .collect()
    }

    #[test]
    fn vr_single_key_no_header() {
        let dm = store_with_keys("msg", &["app.title"]);
        // bundle header + one leaf (chain-collapsed app.title)
        let segs = row_segs(&dm);
        let leafs = row_is_leaf(&dm);
        assert_eq!(segs, vec![vec![], sv(&["app", "title"])]);
        assert_eq!(leafs, vec![false, true]);
    }

    #[test]
    fn vr_siblings_get_header() {
        let dm = store_with_keys("msg", &["app.ok", "app.cancel"]);
        let segs = row_segs(&dm);
        let leafs = row_is_leaf(&dm);
        let indents = row_indents(&dm);
        // bundle header, app header, cancel, ok  (alphabetical)
        assert_eq!(
            segs,
            vec![vec![], sv(&["app"]), sv(&["cancel"]), sv(&["ok"])]
        );
        assert_eq!(leafs, vec![false, false, true, true]);
        assert_eq!(indents, vec![0, 0, 1, 1]);
    }

    #[test]
    fn vr_chain_collapse() {
        // a has one child b which has one child c (a key) → collapses to [a,b,c]
        let dm = store_with_keys("msg", &["a.b.c"]);
        let segs = row_segs(&dm);
        let leafs = row_is_leaf(&dm);
        assert_eq!(segs, vec![vec![], sv(&["a", "b", "c"])]);
        assert_eq!(leafs, vec![false, true]);
    }

    #[test]
    fn vr_key_with_children() {
        // app.x is a key AND has children app.x.y and app.x.z
        let dm = store_with_keys("msg", &["app.x", "app.x.y", "app.x.z"]);
        let segs = row_segs(&dm);
        let leafs = row_is_leaf(&dm);
        let indents = row_indents(&dm);
        // bundle header, app.x (leaf, has_children), x.y, x.z  (alphabetical)
        assert_eq!(leafs, vec![false, true, true, true]);
        assert_eq!(indents, vec![0, 0, 1, 1]);
        // the leaf row carries [app, x], children carry [y] and [z]
        assert_eq!(segs[1], sv(&["app", "x"]));
        assert_eq!(segs[2], sv(&["y"]));
        assert_eq!(segs[3], sv(&["z"]));
    }

    #[test]
    fn vr_filter_suppresses_header() {
        let dm = store_with_keys("msg", &["app.ok", "app.cancel"]);
        // Only "app.ok" passes the filter — header should still appear.
        let ids: Vec<_> = dm
            .visible_rows(|_| true)
            .iter()
            .filter_map(|r| r.key_id)
            .collect();
        let ok_id = ids[0];
        let rows = dm.visible_rows(|kid| kid == ok_id);
        let leafs: Vec<bool> = rows.iter().map(|r| r.key_id.is_some()).collect();
        // bundle header, app header, ok (cancel filtered out)
        assert_eq!(leafs, vec![false, false, true]);
    }

    #[test]
    fn vr_filter_suppresses_group_header() {
        let dm = store_with_keys("msg", &["app.ok", "app.cancel"]);
        // No keys pass → group header suppressed, only bundle header remains.
        let rows = dm.visible_rows(|_| false);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].key_id.is_none());
    }
}
