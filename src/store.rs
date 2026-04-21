//! Central data store for propman.
//!
//! Everything the app needs at runtime lives here: a global segment intern
//! table, a global locale intern table, per-bundle node graphs (the trie
//! stored as flat tables), key entities, their translations, and the filter
//! exception tables (pinned, dirty, temp-pins).
//!
//! ## Design
//!
//! `BundleId`, `NodeId`, and `KeyId` are opaque newtypes — callers never
//! construct them directly. Bundles are created implicitly the first time a
//! locale is registered for them. `KeyId` is returned from `insert_key` and
//! used for all subsequent operations on that key.
//!
//! The only strings that cross the store boundary are at the workspace I/O
//! layer (building from parsed files, flushing to disk). Everything the app
//! does internally — navigation, filtering, sibling detection — works on ids.

use std::collections::{HashMap, HashSet};
use crate::{parser::FileEntry, workspace::Workspace};

// ── ID types ──────────────────────────────────────────────────────────────────
//
// All IDs are opaque newtypes over `u32`.  Callers never construct them —
// they are produced by write operations (`register_locale`, `insert_key`, …)
// and used as handles for subsequent queries.  The private inner field
// prevents external code from treating IDs as plain numbers or accidentally
// mixing ID kinds.
//
// Internally, each type exposes a private `idx(self) -> usize` helper used
// only within this module to index into the backing Vec tables.

/// Globally unique segment string identity.
/// `"app"` gets the same id in every bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalSegmentId(u32);
impl GlobalSegmentId { fn idx(self) -> usize { self.0 as usize } }

/// Globally unique locale string identity (`"de"`, `"fr"`, …).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalLocaleId(u32);
impl GlobalLocaleId { fn idx(self) -> usize { self.0 as usize } }

/// Index into the bundle table. Internal to the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BundleId(u32);
impl BundleId { fn idx(self) -> usize { self.0 as usize } }

/// A node in a bundle's trie (one node = one key segment position).
/// Not comparable across bundles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);
impl NodeId { fn idx(self) -> usize { self.0 as usize } }

/// A translatable entry within a bundle.
/// Not comparable across bundles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyId(u32);
impl KeyId { fn idx(self) -> usize { self.0 as usize } }

/// A locale-specific instantiation of a key: one `(key, locale)` pair that
/// maps to exactly one line in one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntryId(u32);
impl EntryId { fn idx(self) -> usize { self.0 as usize } }

// ── Store ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct Store {
    // ── Global segment intern ─────────────────────────────────────────────────
    seg_strings:    Vec<String>,
    seg_index:      HashMap<String, GlobalSegmentId>,

    // ── Global locale intern ──────────────────────────────────────────────────
    locale_strings: Vec<String>,
    locale_index:   HashMap<String, GlobalLocaleId>,

    // ── Bundles ───────────────────────────────────────────────────────────────
    /// Bundle name as an interned segment. Empty string for bare (no-bundle) keys.
    bundle_name:    Vec<GlobalSegmentId>,
    /// Locales this bundle has a `.properties` file for.
    bundle_locales: Vec<Vec<GlobalLocaleId>>,
    /// Virtual root node for each bundle (no segment, no parent).
    bundle_root:    Vec<NodeId>,
    /// Fast bundle lookup by name segment.
    seg_to_bundle:  HashMap<GlobalSegmentId, BundleId>,

    // ── Trie nodes ────────────────────────────────────────────────────────────
    //
    // One node per unique key-path position within a bundle.
    // The root node per bundle is virtual: no segment, no parent.
    // All other nodes represent one key segment at one depth.
    //
    // Chain-collapse (single-child runs with no keys) is computed on demand,
    // not stored here.

    /// The segment this node represents. `None` for virtual bundle-root nodes.
    node_segment:  Vec<Option<GlobalSegmentId>>,
    /// Parent node. `None` for virtual bundle-root nodes.
    node_parent:   Vec<Option<NodeId>>,
    /// Children indexed by segment for O(1) lookup.
    node_children: Vec<HashMap<GlobalSegmentId, NodeId>>,
    /// Leaf keys whose full path ends exactly at this node.
    node_keys:     Vec<Vec<KeyId>>,
    /// Which bundle this node belongs to.
    node_bundle:   Vec<BundleId>,

    // ── Keys ──────────────────────────────────────────────────────────────────
    /// The node at the end of this key's path.
    key_node:   Vec<NodeId>,
    /// Owning bundle.
    key_bundle: Vec<BundleId>,

    // ── Entry table ───────────────────────────────────────────────────────────
    //
    // One entry per (KeyId, GlobalLocaleId) pair.  Tracks both the original
    // on-disk value (baseline at load time) and the current in-session value.
    // Three disjoint exception sets record what changed since the last save:
    //   inserted_entries – new this session, no original file line
    //   deleted_entries  – existed on disk, removed this session
    //   modified_entries – existed on disk, value changed this session

    /// Which key this entry belongs to.
    entry_key:    Vec<KeyId>,
    /// Which locale this entry belongs to.
    entry_locale: Vec<GlobalLocaleId>,
    /// Value as loaded from disk; `None` for newly-inserted entries.
    entry_original: Vec<Option<String>>,
    /// Current in-session value; `None` for deleted entries (and net-no-op ghost entries).
    entry_current:  Vec<Option<String>>,

    /// Fast `(key, locale) → EntryId` lookup.
    cell_entry:  HashMap<(KeyId, GlobalLocaleId), EntryId>,
    /// All entries for a given key, in insertion order.
    key_entries: HashMap<KeyId, Vec<EntryId>>,

    /// Entries created this session (no original file backing).
    inserted_entries: HashSet<EntryId>,
    /// Entries removed this session (had original file backing).
    deleted_entries:  HashSet<EntryId>,
    /// Entries whose value changed this session (original != current).
    modified_entries: HashSet<EntryId>,

    // ── Exception tables ──────────────────────────────────────────────────────
    pinned:    HashSet<KeyId>,
    temp_pins: HashSet<KeyId>,

    // ── Soft-delete set ───────────────────────────────────────────────────────
    /// Keys that have been logically deleted but whose `KeyId` slots are still
    /// occupied.  `bundle_keys()` filters these out; trie nodes are pruned
    /// immediately in `delete_key`.
    deleted_keys: HashSet<KeyId>,

    // ── Cross-bundle index ────────────────────────────────────────────────────
    /// All nodes that carry a given segment — enables cross-bundle
    /// queries (completion, filter) without scanning every bundle's graph.
    seg_nodes: HashMap<GlobalSegmentId, Vec<NodeId>>,
}

// ── Internal helpers ──────────────────────────────────────────────────────────

impl Store {
    fn intern_segment_mut(&mut self, s: &str) -> GlobalSegmentId {
        if let Some(&id) = self.seg_index.get(s) {
            return id;
        }
        let id = GlobalSegmentId(self.seg_strings.len() as u32);
        self.seg_strings.push(s.to_string());
        self.seg_index.insert(s.to_string(), id);
        id
    }

    fn intern_locale_mut(&mut self, s: &str) -> GlobalLocaleId {
        if let Some(&id) = self.locale_index.get(s) {
            return id;
        }
        let id = GlobalLocaleId(self.locale_strings.len() as u32);
        self.locale_strings.push(s.to_string());
        self.locale_index.insert(s.to_string(), id);
        id
    }

    /// Get the bundle id for `name`, creating the bundle if it does not exist.
    fn get_or_create_bundle(&mut self, name: &str) -> BundleId {
        let name_seg = self.intern_segment_mut(name);
        if let Some(&id) = self.seg_to_bundle.get(&name_seg) {
            return id;
        }
        let bundle_id = BundleId(self.bundle_name.len() as u32);
        let root_id   = self.alloc_node(None, None, bundle_id);

        self.bundle_name.push(name_seg);
        self.bundle_locales.push(Vec::new());
        self.bundle_root.push(root_id);
        self.seg_to_bundle.insert(name_seg, bundle_id);

        bundle_id
    }

    fn alloc_entry(&mut self, key_id: KeyId, locale_id: GlobalLocaleId) -> EntryId {
        let eid = EntryId(self.entry_key.len() as u32);
        self.entry_key.push(key_id);
        self.entry_locale.push(locale_id);
        self.entry_original.push(None);
        self.entry_current.push(None);
        self.cell_entry.insert((key_id, locale_id), eid);
        self.key_entries.entry(key_id).or_default().push(eid);
        eid
    }

    fn alloc_node(
        &mut self,
        segment: Option<GlobalSegmentId>,
        parent:  Option<NodeId>,
        bundle:  BundleId,
    ) -> NodeId {
        let id = NodeId(self.node_segment.len() as u32);
        self.node_segment.push(segment);
        self.node_parent.push(parent);
        self.node_children.push(HashMap::new());
        self.node_keys.push(Vec::new());
        self.node_bundle.push(bundle);
        id
    }
}

// ── Write operations ──────────────────────────────────────────────────────────

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a locale file for a bundle.
    ///
    /// Creates the bundle if it does not exist yet. Safe to call multiple
    /// times — duplicate locales are ignored.
    pub fn register_locale(&mut self, bundle: &str, locale: &str) {
        let bundle_id = self.get_or_create_bundle(bundle);
        let locale_id = self.intern_locale_mut(locale);
        let locales   = &mut self.bundle_locales[bundle_id.idx()];
        if !locales.contains(&locale_id) {
            locales.push(locale_id);
        }
    }

    /// Get or insert a key for `bundle` / `real_key`.
    ///
    /// `real_key` is the bare dot-separated key (no bundle prefix). If the key
    /// already exists in the bundle the existing `KeyId` is returned, making
    /// this safe to call once per locale file without producing duplicates.
    pub fn insert_key(&mut self, bundle: &str, real_key: &str) -> KeyId {
        let bundle_id = self.get_or_create_bundle(bundle);
        let segments: Vec<GlobalSegmentId> = real_key
            .split('.')
            .map(|s| self.intern_segment_mut(s))
            .collect();

        let mut node = self.bundle_root[bundle_id.idx()];

        for &seg in &segments {
            node = if let Some(&child) = self.node_children[node.idx()].get(&seg) {
                child
            } else {
                let child = self.alloc_node(Some(seg), Some(node), bundle_id);
                self.node_children[node.idx()].insert(seg, child);
                self.seg_nodes.entry(seg).or_default().push(child);
                child
            };
        }

        // Return existing key if already registered at this node.
        if let Some(&existing) = self.node_keys[node.idx()].first() {
            return existing;
        }

        let key_id = KeyId(self.key_node.len() as u32);
        self.key_node.push(node);
        self.key_bundle.push(bundle_id);
        self.node_keys[node.idx()].push(key_id);
        key_id
    }

    /// Set or update a translation at mutation time (user edit/insert).
    /// Creates a new entry if none exists; otherwise updates the current value
    /// and adjusts the appropriate exception set.
    pub fn set_translation(&mut self, key_id: KeyId, locale: &str, value: String) {
        let locale_id = self.intern_locale_mut(locale);

        if let Some(&eid) = self.cell_entry.get(&(key_id, locale_id)) {
            // Remove from deleted (user is re-adding a deleted entry).
            self.deleted_entries.remove(&eid);
            self.entry_current[eid.idx()] = Some(value.clone());

            if self.inserted_entries.contains(&eid) {
                // Still a new entry — nothing else to track.
            } else {
                // Update modified state vs original.
                match &self.entry_original[eid.idx()] {
                    Some(orig) if orig == &value => { self.modified_entries.remove(&eid); }
                    _ => { self.modified_entries.insert(eid); }
                }
            }
        } else {
            let eid = self.alloc_entry(key_id, locale_id);
            self.entry_current[eid.idx()] = Some(value);
            self.inserted_entries.insert(eid);
        }
    }

    /// Load a translation at startup (no change recorded).
    /// Sets both `entry_original` and `entry_current` to the given value so
    /// the baseline matches the on-disk content.
    pub fn load_translation(&mut self, key_id: KeyId, locale: &str, value: String) {
        let locale_id = self.intern_locale_mut(locale);
        let eid = if let Some(&e) = self.cell_entry.get(&(key_id, locale_id)) {
            e
        } else {
            self.alloc_entry(key_id, locale_id)
        };
        self.entry_original[eid.idx()] = Some(value.clone());
        self.entry_current[eid.idx()]  = Some(value);
    }

    /// Remove a translation (locale cell deleted by the user).
    pub fn remove_translation(&mut self, key_id: KeyId, locale: &str) {
        let locale_id = match self.locale_index.get(locale).copied() {
            Some(lid) => lid,
            None => return,
        };
        let eid = match self.cell_entry.get(&(key_id, locale_id)).copied() {
            Some(e) => e,
            None => return,
        };
        if self.inserted_entries.contains(&eid) {
            // Net no-op: inserted then deleted in same session — ghost entry.
            self.inserted_entries.remove(&eid);
        } else {
            self.deleted_entries.insert(eid);
            self.modified_entries.remove(&eid);
        }
        self.entry_current[eid.idx()] = None;
    }

    /// Permanently remove a key from the store.
    ///
    /// - Removes the key from its node's key list.
    /// - Marks all entries for this key as deleted (or cancels inserted ones).
    /// - Prunes orphan trie nodes bottom-up so chain-collapse remains accurate.
    /// - Marks the key in `deleted_keys` so `bundle_keys()` skips it.
    /// - Removes the key from exception sets (pinned, temp_pins).
    pub fn delete_key(&mut self, key_id: KeyId) {
        self.deleted_keys.insert(key_id);

        // Remove from node_keys at the leaf node.
        let node = self.key_node[key_id.idx()];
        self.node_keys[node.idx()].retain(|&k| k != key_id);

        // Mark all entries for this key as deleted (or cancel net-no-op inserts).
        let entry_ids: Vec<EntryId> = self.key_entries
            .get(&key_id)
            .cloned()
            .unwrap_or_default();
        for eid in entry_ids {
            if self.inserted_entries.contains(&eid) {
                // Net no-op: inserted then deleted in same session.
                self.inserted_entries.remove(&eid);
            } else {
                self.deleted_entries.insert(eid);
                self.modified_entries.remove(&eid);
            }
            self.entry_current[eid.idx()] = None;
        }

        // Prune empty nodes upward: if a node has no keys and no children it
        // is an orphan — remove it from the parent's child map and seg_nodes.
        let mut prune = node;
        loop {
            if !self.node_keys[prune.idx()].is_empty() { break; }
            if !self.node_children[prune.idx()].is_empty() { break; }
            let seg    = match self.node_segment[prune.idx()] { Some(s) => s, None => break };
            let parent = match self.node_parent[prune.idx()]  { Some(p) => p, None => break };
            self.node_children[parent.idx()].remove(&seg);
            if let Some(nodes) = self.seg_nodes.get_mut(&seg) {
                nodes.retain(|&n| n != prune);
            }
            prune = parent;
        }

        // Clear exception flags.
        self.pinned.remove(&key_id);
        self.temp_pins.remove(&key_id);
    }
}

// ── Read accessors ────────────────────────────────────────────────────────────
//
// Store query API — called by DomainModel handle methods once implemented.
// See docs/architecture.md "Handle types" and docs/architectural_debt.md D2.

#[allow(dead_code)]
impl Store {
    pub fn segment_str(&self, id: GlobalSegmentId) -> &str {
        &self.seg_strings[id.idx()]
    }

    pub fn locale_str(&self, id: GlobalLocaleId) -> &str {
        &self.locale_strings[id.idx()]
    }

    /// Iterate over all bundle ids in insertion order.
    pub fn bundle_ids(&self) -> impl Iterator<Item = BundleId> {
        (0..self.bundle_name.len()).map(|i| BundleId(i as u32))
    }

    pub fn bundle_name_str(&self, bundle_id: BundleId) -> &str {
        self.segment_str(self.bundle_name[bundle_id.idx()])
    }

    pub fn bundle_locales(&self, bundle_id: BundleId) -> &[GlobalLocaleId] {
        &self.bundle_locales[bundle_id.idx()]
    }

    pub fn bundle_root(&self, bundle_id: BundleId) -> NodeId {
        self.bundle_root[bundle_id.idx()]
    }

    pub fn translation(&self, key_id: KeyId, locale_id: GlobalLocaleId) -> Option<&str> {
        let &eid = self.cell_entry.get(&(key_id, locale_id))?;
        self.entry_current[eid.idx()].as_deref()
    }

    /// Look up a translation by locale name string.
    ///
    /// Returns `None` when the locale is not registered or the key has no
    /// translation for it.  Use `translation` when you already have a
    /// `GlobalLocaleId`; this variant accepts a name string for callers that
    /// have not yet resolved the locale to an ID.
    pub fn translation_for_str(&self, key_id: KeyId, locale: &str) -> Option<&str> {
        let &locale_id = self.locale_index.get(locale)?;
        let &eid = self.cell_entry.get(&(key_id, locale_id))?;
        self.entry_current[eid.idx()].as_deref()
    }

    pub fn node_segment(&self, node: NodeId) -> Option<GlobalSegmentId> {
        self.node_segment[node.idx()]
    }

    pub fn node_parent(&self, node: NodeId) -> Option<NodeId> {
        self.node_parent[node.idx()]
    }

    pub fn node_children(&self, node: NodeId) -> &HashMap<GlobalSegmentId, NodeId> {
        &self.node_children[node.idx()]
    }

    pub fn node_keys(&self, node: NodeId) -> &[KeyId] {
        &self.node_keys[node.idx()]
    }

    pub fn node_bundle(&self, node: NodeId) -> BundleId {
        self.node_bundle[node.idx()]
    }

    pub fn key_node(&self, key_id: KeyId) -> NodeId {
        self.key_node[key_id.idx()]
    }

    pub fn key_bundle(&self, key_id: KeyId) -> BundleId {
        self.key_bundle[key_id.idx()]
    }

    /// Number of segments from the bundle root to `nid` (root = depth 0).
    pub fn node_depth(&self, nid: NodeId) -> usize {
        let mut depth = 0;
        let mut cur = nid;
        while let Some(parent) = self.node_parent[cur.idx()] {
            depth += 1;
            cur = parent;
        }
        depth
    }

    /// Ancestor of `nid` at exactly `target_depth` segments from the bundle root.
    /// Returns `nid` unchanged when `target_depth >= node_depth(nid)`.
    pub fn node_ancestor_at_depth(&self, nid: NodeId, target_depth: usize) -> NodeId {
        let steps = self.node_depth(nid).saturating_sub(target_depth);
        let mut cur = nid;
        for _ in 0..steps {
            if let Some(p) = self.node_parent[cur.idx()] { cur = p; }
        }
        cur
    }

    /// `true` when `ancestor` is a strict (not equal) ancestor of `nid`.
    pub fn node_is_strict_ancestor(&self, ancestor: NodeId, nid: NodeId) -> bool {
        let mut cur = nid;
        while let Some(parent) = self.node_parent[cur.idx()] {
            if parent == ancestor { return true; }
            cur = parent;
        }
        false
    }

    /// Bundle-qualified string for trie node `nid`.
    /// Root of a named bundle → `"messages"`. Interior/leaf → `"messages:app.confirm"`.
    pub fn node_qualified_str(&self, nid: NodeId) -> String {
        let mut cur = nid;
        let mut segs: Vec<&str> = Vec::new();
        while let Some(seg) = self.node_segment[cur.idx()] {
            segs.push(self.segment_str(seg));
            match self.node_parent[cur.idx()] {
                Some(p) => cur = p,
                None    => break,
            }
        }
        segs.reverse();
        let bundle   = self.bundle_name_str(self.node_bundle[nid.idx()]);
        let key_part = segs.join(".");
        if bundle.is_empty() {
            key_part
        } else if key_part.is_empty() {
            bundle.to_string()
        } else {
            format!("{bundle}:{key_part}")
        }
    }

    /// Full segment path for a key (root → leaf order).
    pub fn key_segments(&self, key_id: KeyId) -> Vec<GlobalSegmentId> {
        let mut node = self.key_node[key_id.idx()];
        let mut segs = Vec::new();
        while let Some(seg) = self.node_segment[node.idx()] {
            segs.push(seg);
            node = match self.node_parent[node.idx()] {
                Some(p) => p,
                None    => break,
            };
        }
        segs.reverse();
        segs
    }

    /// Dot-joined real key string (no bundle prefix). Workspace boundary only.
    pub fn key_real_key_str(&self, key_id: KeyId) -> String {
        self.key_segments(key_id)
            .iter()
            .map(|&s| self.segment_str(s))
            .collect::<Vec<_>>()
            .join(".")
    }

    /// Bundle-qualified key string. Workspace boundary and display only.
    pub fn key_qualified_str(&self, key_id: KeyId) -> String {
        let bundle = self.bundle_name_str(self.key_bundle[key_id.idx()]);
        let real   = self.key_real_key_str(key_id);
        if bundle.is_empty() { real } else { format!("{bundle}:{real}") }
    }

    /// All key ids belonging to `bundle_id`, in insertion order.
    /// Deleted keys are excluded.
    pub fn bundle_keys(&self, bundle_id: BundleId) -> impl Iterator<Item = KeyId> + '_ {
        (0..self.key_bundle.len())
            .map(|i| KeyId(i as u32))
            .filter(move |&k| {
                self.key_bundle[k.idx()] == bundle_id && !self.deleted_keys.contains(&k)
            })
    }

    /// Look up a key by bundle name and real (dot-joined) key string.
    /// Returns `None` if the key was never inserted or has been deleted.
    pub fn find_key(&self, bundle: &str, real_key: &str) -> Option<KeyId> {
        let &bundle_seg = self.seg_index.get(bundle)?;
        let &bundle_id  = self.seg_to_bundle.get(&bundle_seg)?;
        let mut node = self.bundle_root[bundle_id.idx()];
        for seg_str in real_key.split('.') {
            let &seg   = self.seg_index.get(seg_str)?;
            let &child = self.node_children[node.idx()].get(&seg)?;
            node = child;
        }
        // Return the first non-deleted key at this node (in practice there is at
        // most one, but we guard against the deleted_keys set just in case).
        self.node_keys[node.idx()].iter()
            .find(|&&k| !self.deleted_keys.contains(&k))
            .copied()
    }

    /// Find a bundle by its name string.
    /// Returns `None` when no bundle with that name exists.
    pub fn find_bundle(&self, name: &str) -> Option<BundleId> {
        let &seg = self.seg_index.get(name)?;
        self.seg_to_bundle.get(&seg).copied()
    }

    /// Returns `true` when `bundle_id` has a locale file for `locale`.
    pub fn bundle_has_locale_str(&self, bundle_id: BundleId, locale: &str) -> bool {
        match self.locale_index.get(locale) {
            Some(&lid) => self.bundle_locales[bundle_id.idx()].contains(&lid),
            None       => false,
        }
    }

    // ── Pinning ───────────────────────────────────────────────────────────────

    pub fn is_pinned(&self, key_id: KeyId) -> bool {
        self.pinned.contains(&key_id)
    }
    pub fn pin_key(&mut self, key_id: KeyId) {
        self.pinned.insert(key_id);
    }
    pub fn unpin_key(&mut self, key_id: KeyId) {
        self.pinned.remove(&key_id);
    }

    // ── Temp-pins ─────────────────────────────────────────────────────────────

    pub fn is_temp_pinned(&self, key_id: KeyId) -> bool {
        self.temp_pins.contains(&key_id)
    }
    pub fn has_temp_pins(&self) -> bool {
        !self.temp_pins.is_empty()
    }
    pub fn set_temp_pins(&mut self, pins: Vec<KeyId>) {
        self.temp_pins.clear();
        self.temp_pins.extend(pins);
    }
    pub fn clear_temp_pins(&mut self) {
        self.temp_pins.clear();
    }
    pub fn temp_pins_match(&self, candidate: &[KeyId]) -> bool {
        if candidate.len() != self.temp_pins.len() { return false; }
        candidate.iter().all(|k| self.temp_pins.contains(k))
    }

    /// All non-deleted key IDs across all bundles, in insertion order.
    pub fn all_key_ids(&self) -> impl Iterator<Item = KeyId> + '_ {
        (0..self.key_node.len())
            .map(|i| KeyId(i as u32))
            .filter(move |k| !self.deleted_keys.contains(k))
    }

    /// All nodes that carry `seg` — cross-bundle index.
    pub fn nodes_for_segment(&self, seg: GlobalSegmentId) -> &[NodeId] {
        self.seg_nodes.get(&seg).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Chain-collapsed segment groups for `key_id`, in root-to-leaf order.
    ///
    /// Each inner `Vec` is one partition — a run of nodes that form a
    /// single-child chain with no keys between two branch points (chain-collapse).
    ///
    /// Example: `app.dialog.yes` where `app` has one child `dialog` which has
    /// two children → `[[app, dialog], [yes]]`.
    ///
    /// Use `node_display_chain` when you only need the segments since the last
    /// branch, not the full root-to-leaf path.
    pub fn key_compressed(&self, key_id: KeyId) -> Vec<Vec<GlobalSegmentId>> {
        // Walk leaf → root collecting the node path, then reverse.
        let mut node = self.key_node[key_id.idx()];
        let mut path: Vec<NodeId> = Vec::new();
        loop {
            if self.node_segment[node.idx()].is_none() { break; } // hit bundle root
            path.push(node);
            match self.node_parent[node.idx()] {
                Some(p) => node = p,
                None    => break,
            }
        }
        path.reverse();

        // Group consecutive nodes that form a single-child chain with no keys.
        let mut groups: Vec<Vec<GlobalSegmentId>> = Vec::new();
        let mut current: Vec<GlobalSegmentId>     = Vec::new();

        for &n in &path {
            let seg = self.node_segment[n.idx()].unwrap(); // safe: root filtered above
            current.push(seg);
            let is_chain = self.node_children[n.idx()].len() == 1
                && self.node_keys[n.idx()].is_empty();
            if !is_chain {
                groups.push(std::mem::take(&mut current));
            }
        }

        groups
    }

    /// Segment chain for the row anchored at `node`, in root-to-leaf order.
    ///
    /// Walks from `node` toward the bundle root, collecting segments as long as
    /// each parent is chain-collapsible (exactly one child, no key, has a
    /// segment).  Stops climbing when the parent is a branch, a key node, or
    /// the virtual bundle root.  The collected nodes are reversed to give
    /// root-to-leaf order.
    ///
    /// Returns only the segments since the last branch point, not the full
    /// root-to-leaf path.
    pub fn node_display_chain(&self, node: NodeId) -> Vec<GlobalSegmentId> {
        let mut current = node;
        let mut chain: Vec<GlobalSegmentId> = Vec::new();

        loop {
            let seg = match self.node_segment[current.idx()] {
                Some(s) => s,
                None    => break, // bundle root — stop
            };
            chain.push(seg);

            let parent = match self.node_parent[current.idx()] {
                Some(p) => p,
                None    => break,
            };

            // Climb into parent only when it is chain-collapsible:
            //   one child, no key at parent, parent has a segment.
            let parent_is_chain = self.node_children[parent.idx()].len() == 1
                && self.node_keys[parent.idx()].is_empty()
                && self.node_segment[parent.idx()].is_some();

            if parent_is_chain {
                current = parent;
            } else {
                break;
            }
        }

        chain.reverse();
        chain
    }

    /// Create a read-only handle for `key_id`.
    pub fn key_handle(&self, key_id: KeyId) -> KeyHandle<'_> {
        KeyHandle { id: key_id, store: self }
    }

    /// Iterate all non-deleted keys across all bundles as read-only handles.
    pub fn all_key_handles(&self) -> impl Iterator<Item = KeyHandle<'_>> {
        let len = self.key_node.len();
        (0..len)
            .map(|i| KeyId(i as u32))
            .filter(move |k| !self.deleted_keys.contains(k))
            .map(move |k| KeyHandle { id: k, store: self })
    }
}

// ── KeyHandle ─────────────────────────────────────────────────────────────────

/// Short-lived read handle for a single key.
///
/// Created inside ops for the duration of a read; dropped before any
/// `&mut DomainModel` call. See docs/architecture.md "Handle types".
#[allow(dead_code)]
pub struct KeyHandle<'a> {
    id:    KeyId,
    store: &'a Store,
}

#[allow(dead_code)]
impl<'a> KeyHandle<'a> {
    pub fn id(&self) -> KeyId { self.id }

    pub fn bundle_name(&self) -> &'a str {
        self.store.bundle_name_str(self.store.key_bundle[self.id.idx()])
    }

    pub fn real_key_str(&self) -> String {
        self.store.key_real_key_str(self.id)
    }

    pub fn qualified_str(&self) -> String {
        self.store.key_qualified_str(self.id)
    }

    /// Chain-collapsed segment groups, resolved to string slices.
    pub fn compressed(&self) -> Vec<Vec<&'a str>> {
        self.store.key_compressed(self.id)
            .into_iter()
            .map(|g| g.into_iter().map(|s| self.store.segment_str(s)).collect())
            .collect()
    }

    pub fn translation(&self, locale: &str) -> Option<&'a str> {
        self.store.translation_for_str(self.id, locale)
    }

    /// `true` when this key has no translations in any of its bundle's locales.
    pub fn is_dangling(&self) -> bool {
        let bundle_id = self.store.key_bundle[self.id.idx()];
        self.store.bundle_locales[bundle_id.idx()]
            .iter()
            .all(|&lid| self.store.translation(self.id, lid).is_none())
    }

    /// `true` when this key's node has at least one child (deeper key exists).
    pub fn has_children(&self) -> bool {
        let node = self.store.key_node[self.id.idx()];
        !self.store.node_children[node.idx()].is_empty()
    }

    pub fn is_dirty(&self) -> bool {
        self.store.is_dirty(self.id)
    }
}

// ── Change-set queries ────────────────────────────────────────────────────────

/// One pending change derived from the entry exception sets.
#[derive(Debug)]
pub enum Change {
    /// A brand-new entry; no original file line — workspace must insert it.
    Insert { entry_id: EntryId },
    /// An existing entry whose value changed — workspace must rewrite the line.
    Update { entry_id: EntryId },
    /// An existing entry that was removed — workspace must delete the line(s).
    Delete { entry_id: EntryId },
}

impl Store {
    /// `true` when there are any unsaved changes (inserts, updates, or deletes).
    pub fn has_changes(&self) -> bool {
        !self.inserted_entries.is_empty()
            || !self.deleted_entries.is_empty()
            || !self.modified_entries.is_empty()
    }

    /// `true` when the specific `(key_id, locale)` entry has unsaved changes.
    pub fn is_dirty_for_locale(&self, key_id: KeyId, locale: &str) -> bool {
        let Some(&locale_id) = self.locale_index.get(locale) else { return false; };
        let Some(&eid) = self.cell_entry.get(&(key_id, locale_id)) else { return false; };
        self.inserted_entries.contains(&eid)
            || self.modified_entries.contains(&eid)
            || self.deleted_entries.contains(&eid)
    }

    /// `true` when `key_id` has any changed entries (dirty in the UI sense).
    pub fn is_dirty(&self, key_id: KeyId) -> bool {
        self.key_entries
            .get(&key_id)
            .map(|eids| eids.iter().any(|eid| {
                self.inserted_entries.contains(eid)
                    || self.deleted_entries.contains(eid)
                    || self.modified_entries.contains(eid)
            }))
            .unwrap_or(false)
    }

    /// Iterator over all pending changes (inserts, updates, deletes).
    /// Used by `workspace.save()` to derive file writes.
    pub fn change_set(&self) -> impl Iterator<Item = Change> + '_ {
        let inserted = self.inserted_entries.iter().map(|&eid| Change::Insert { entry_id: eid });
        let updated  = self.modified_entries.iter().map(|&eid| Change::Update { entry_id: eid });
        let deleted  = self.deleted_entries.iter().map(|&eid| Change::Delete  { entry_id: eid });
        inserted.chain(updated).chain(deleted)
    }

    /// Reset the change log after a successful save.
    /// Copies `entry_current` into `entry_original` so the new on-disk state
    /// becomes the new baseline; clears all exception sets.
    pub fn clear_changes(&mut self) {
        for eid in self.inserted_entries.drain() {
            self.entry_original[eid.idx()] = self.entry_current[eid.idx()].clone();
        }
        for eid in self.modified_entries.drain() {
            self.entry_original[eid.idx()] = self.entry_current[eid.idx()].clone();
        }
        self.deleted_entries.clear();
    }

    // ── Entry accessors for workspace.save() ─────────────────────────────────

    pub fn entry_key_id(&self, eid: EntryId) -> KeyId {
        self.entry_key[eid.idx()]
    }

    pub fn entry_locale_id(&self, eid: EntryId) -> GlobalLocaleId {
        self.entry_locale[eid.idx()]
    }

    pub fn entry_current_value(&self, eid: EntryId) -> Option<&str> {
        self.entry_current[eid.idx()].as_deref()
    }
}

// ── Build from workspace ──────────────────────────────────────────────────────

impl Store {
    /// Build a `Store` from the parsed workspace.
    ///
    /// The workspace is the I/O source of truth; this consumes its data and
    /// produces the store that the rest of the app queries. After this call the
    /// workspace is only needed for write-back (saving changes to disk via
    /// the store's change_set()).
    pub fn from_workspace(ws: &Workspace) -> Self {
        let mut store = Store::new();

        for group in &ws.groups {
            // Register locale files before inserting keys so the bundle exists.
            for file in &group.files {
                store.register_locale(&group.base_name, &file.locale);
            }

            // Insert keys and their translations.
            // `insert_key` is idempotent so encountering the same key in multiple
            // locale files is safe — the second call returns the existing KeyId.
            for file in &group.files {
                for entry in &file.entries {
                    if let FileEntry::KeyValue { key, value, .. } = entry {
                        let key_id = store.insert_key(&group.base_name, key);
                        store.load_translation(key_id, &file.locale, value.clone());
                    }
                }
            }
        }

        store
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_segments_are_deduplicated() {
        let mut s = Store::new();
        let a = s.intern_segment_mut("app");
        let b = s.intern_segment_mut("app");
        assert_eq!(a, b);
        assert_eq!(s.segment_str(a), "app");
    }

    #[test]
    fn register_locale_creates_bundle() {
        let mut s = Store::new();
        s.register_locale("messages", "de");
        s.register_locale("messages", "fr");

        let ids: Vec<BundleId> = s.bundle_ids().collect();
        assert_eq!(ids.len(), 1);
        assert_eq!(s.bundle_name_str(ids[0]), "messages");
        assert_eq!(s.bundle_locales(ids[0]).len(), 2);
    }

    #[test]
    fn register_locale_is_idempotent() {
        let mut s = Store::new();
        s.register_locale("messages", "de");
        s.register_locale("messages", "de");
        let b = s.bundle_ids().next().unwrap();
        assert_eq!(s.bundle_locales(b).len(), 1);
    }

    #[test]
    fn insert_key_builds_partition_graph() {
        let mut s = Store::new();
        s.register_locale("messages", "de");
        let k = s.insert_key("messages", "app.confirm.title");

        let node = s.key_node(k);
        assert_eq!(s.node_segment(node), Some(s.seg_index["title"]));

        let parent = s.node_parent(node).unwrap();
        assert_eq!(s.node_segment(parent), Some(s.seg_index["confirm"]));

        assert_eq!(
            s.key_segments(k).iter().map(|&id| s.segment_str(id)).collect::<Vec<_>>(),
            vec!["app", "confirm", "title"]
        );
    }

    #[test]
    fn insert_key_is_idempotent() {
        let mut s = Store::new();
        s.register_locale("messages", "de");
        let k1 = s.insert_key("messages", "app.title");
        let k2 = s.insert_key("messages", "app.title");
        assert_eq!(k1, k2);
    }

    #[test]
    fn shared_prefix_reuses_partition_nodes() {
        let mut s = Store::new();
        s.register_locale("messages", "de");
        let k1 = s.insert_key("messages", "app.confirm.ok");
        let k2 = s.insert_key("messages", "app.confirm.cancel");

        // Both keys share the "confirm" node as parent.
        let parent1 = s.node_parent(s.key_node(k1)).unwrap();
        let parent2 = s.node_parent(s.key_node(k2)).unwrap();
        assert_eq!(parent1, parent2);
    }

    #[test]
    fn translation_roundtrip() {
        let mut s = Store::new();
        s.register_locale("messages", "de");
        let k = s.insert_key("messages", "app.title");
        s.set_translation(k, "de", "Titel".to_string());

        let de = s.locale_index["de"];
        assert_eq!(s.translation(k, de), Some("Titel"));
    }

    #[test]
    fn qualified_key_string() {
        let mut s = Store::new();
        s.register_locale("messages", "de");
        let k = s.insert_key("messages", "app.title");
        assert_eq!(s.key_qualified_str(k), "messages:app.title");
    }

    #[test]
    fn bare_key_bundle() {
        let mut s = Store::new();
        s.register_locale("", "default");
        let k = s.insert_key("", "loading");
        assert_eq!(s.key_qualified_str(k), "loading");
    }

    #[test]
    fn cross_bundle_segment_index() {
        let mut s = Store::new();
        s.register_locale("messages", "de");
        s.register_locale("errors",   "de");
        s.insert_key("messages", "app.title");
        s.insert_key("errors",   "app.message");

        let app_id = s.seg_index["app"];
        assert_eq!(s.nodes_for_segment(app_id).len(), 2);
    }

    #[test]
    fn two_bundles_are_separate() {
        let mut s = Store::new();
        s.register_locale("messages", "de");
        s.register_locale("errors",   "de");
        let ids: Vec<BundleId> = s.bundle_ids().collect();
        assert_eq!(ids.len(), 2);
        assert_eq!(s.bundle_name_str(ids[0]), "messages");
        assert_eq!(s.bundle_name_str(ids[1]), "errors");
    }

    #[test]
    fn key_compressed_chain_collapse() {
        // app.dialog.yes + app.dialog.no
        // app has 1 child (dialog), dialog has 2 children → [app,dialog] collapses
        let mut s = Store::new();
        s.register_locale("msg", "de");
        let yes = s.insert_key("msg", "app.dialog.yes");
        let no  = s.insert_key("msg", "app.dialog.no");

        let yes_parts = s.key_compressed(yes);
        let no_parts  = s.key_compressed(no);

        // yes: [[app, dialog], [yes]]
        assert_eq!(yes_parts.len(), 2);
        assert_eq!(yes_parts[0].iter().map(|&id| s.segment_str(id)).collect::<Vec<_>>(),
                   vec!["app", "dialog"]);
        assert_eq!(yes_parts[1].iter().map(|&id| s.segment_str(id)).collect::<Vec<_>>(),
                   vec!["yes"]);

        // no: [[app, dialog], [no]]
        assert_eq!(no_parts.len(), 2);
        assert_eq!(no_parts[1].iter().map(|&id| s.segment_str(id)).collect::<Vec<_>>(),
                   vec!["no"]);
    }

    #[test]
    fn key_compressed_no_collapse() {
        // Single key with no siblings anywhere → whole path collapses into one group
        let mut s = Store::new();
        s.register_locale("msg", "de");
        let k = s.insert_key("msg", "app.title");
        let parts = s.key_compressed(k);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].iter().map(|&id| s.segment_str(id)).collect::<Vec<_>>(),
                   vec!["app", "title"]);
    }

    #[test]
    fn find_key_existing() {
        let mut s = Store::new();
        s.register_locale("msg", "de");
        let k = s.insert_key("msg", "app.title");
        assert_eq!(s.find_key("msg", "app.title"), Some(k));
    }

    #[test]
    fn find_key_missing() {
        let mut s = Store::new();
        s.register_locale("msg", "de");
        s.insert_key("msg", "app.title");
        assert_eq!(s.find_key("msg", "app.subtitle"), None);
        assert_eq!(s.find_key("other", "app.title"),  None);
    }

    #[test]
    fn delete_key_removes_from_bundle_keys() {
        let mut s = Store::new();
        s.register_locale("msg", "de");
        let k1 = s.insert_key("msg", "app.title");
        let k2 = s.insert_key("msg", "app.body");
        let b  = s.bundle_ids().next().unwrap();

        s.delete_key(k1);

        let remaining: Vec<KeyId> = s.bundle_keys(b).collect();
        assert!(!remaining.contains(&k1));
        assert!(remaining.contains(&k2));
    }

    #[test]
    fn delete_key_removes_translations() {
        let mut s = Store::new();
        s.register_locale("msg", "de");
        let k = s.insert_key("msg", "app.title");
        s.set_translation(k, "de", "Titel".to_string());

        let de = s.locale_index["de"];
        assert!(s.translation(k, de).is_some());

        s.delete_key(k);
        assert!(s.translation(k, de).is_none());
    }

    #[test]
    fn delete_key_prunes_orphan_nodes_restoring_chain_collapse() {
        // Before delete: app has 2 children (b and d) → no chain-collapse.
        // After deleting app.b.c (the only key under b), b's branch is orphaned and
        // removed.  Now app has 1 child (d) → app.d should chain-collapse.
        let mut s = Store::new();
        s.register_locale("msg", "de");
        let k_bc = s.insert_key("msg", "app.b.c");
        let k_d  = s.insert_key("msg", "app.d");

        // Before delete: app.d compressed as [[app], [d]] (app has 2 children).
        let parts_before = s.key_compressed(k_d);
        assert_eq!(parts_before.len(), 2,
            "expected no collapse before delete — app has 2 children");

        s.delete_key(k_bc);

        // After delete: app.b.c is gone, b is pruned → app.d collapses to [[app, d]].
        let parts_after = s.key_compressed(k_d);
        assert_eq!(parts_after.len(), 1,
            "expected chain-collapse after delete — app now has 1 child");
        assert_eq!(
            parts_after[0].iter().map(|&id| s.segment_str(id)).collect::<Vec<_>>(),
            vec!["app", "d"]
        );
    }

    #[test]
    fn delete_key_find_key_returns_none_after_delete() {
        let mut s = Store::new();
        s.register_locale("msg", "de");
        let k = s.insert_key("msg", "app.title");
        assert!(s.find_key("msg", "app.title").is_some());
        s.delete_key(k);
        assert_eq!(s.find_key("msg", "app.title"), None);
    }
}
