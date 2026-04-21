# The propman Store

The `Store` (`src/store.rs`) is the central data model for propman. It holds
everything the app needs at runtime: the key hierarchy, translations, locale
registrations, and mutation state. It is the result of several iterations on
finding the right data model — earlier versions used flat key lists with the
trie as a rendering aid. Here the trie **is** the data model.

---

## Design principles

**The trie is load-bearing, not decorative.**
Chain-collapse, subtree rename, header suppression, and filter integration all
fall out of the trie structure naturally. They are not special cases bolted on.

**Strings only cross the boundary at I/O time.**
Every internal operation — navigation, filtering, sibling detection, chain-
collapse — works on typed IDs. String resolution only happens when displaying
to the user or writing to disk.

**The store knows nothing about the UI.**
It has no concept of filter expressions, cursor position, edit modes, or
write queues. Those live in `AppState` / `DomainModel`. The store is a
pure data structure with a query and mutation API.

**IDs stay inside the store and domain model.**
Everything outside `store.rs` and `domain.rs` works with *handles* — thin
newtypes over IDs that expose typed methods (`handle.name()`, `handle.bundle()`,
`handle.translation(locale)`). This hides the ID representation and prevents
callers from manufacturing IDs from raw integers. See `docs/architecture.md`
for the full handle-type table.

**The store owns mutation tracking; `PendingChange` is being retired.**
The old design accumulated `PendingChange` entries in `AppState` and flushed
them on Ctrl+S. The new design records all in-session mutations inside the
store (`mutations`, `deleted`, `segment_renames`). At save time,
`workspace.save(&domain_model)` reads the store's change set and derives
the minimal file writes itself. Ops no longer queue writes — they only call
`DomainModel` methods.

---

## ID types

All five ID types are opaque newtypes over `u32`:

```rust
pub struct GlobalSegmentId(u32);  // "app" gets the same id in every bundle
pub struct GlobalLocaleId(u32);   // "de", "fr", …
pub struct BundleId(u32);         // index into the bundle table
pub struct NodeId(u32);           // a node in a bundle's trie
pub struct KeyId(u32);            // a translatable entry within a bundle
```

**Why newtypes, not type aliases?**
`type KeyId = u32` is invisible to the compiler — you can pass a `NodeId`
where a `KeyId` is expected and nothing complains. With newtypes, mixing ID
kinds is a compile error. The private inner field also means external code
cannot fabricate IDs from raw numbers; they can only be obtained from the
store's write operations.

**Internal indexing** uses a private `idx(self) -> usize` method on each type,
defined only inside `store.rs`. All `vec[id.idx()]` patterns live there. If
the backing type ever needs to widen to `u64`, there is one place to change.

**Callers never construct IDs.** They are produced by `register_locale` and
`insert_key`, then used as opaque handles for subsequent queries.

---

## Internal layout

The store uses parallel `Vec` tables — one entry per entity, indexed by ID:

```
Bundles
  bundle_name:    Vec<GlobalSegmentId>         bundle_id → name segment
  bundle_locales: Vec<Vec<GlobalLocaleId>>     bundle_id → registered locales
  bundle_root:    Vec<NodeId>                  bundle_id → virtual root node
  seg_to_bundle:  HashMap<GlobalSegmentId, BundleId>   fast name lookup

Trie nodes  (one per key-path position within a bundle)
  node_segment:   Vec<Option<GlobalSegmentId>> None for virtual root nodes
  node_parent:    Vec<Option<NodeId>>          None for virtual root nodes
  node_children:  Vec<HashMap<GlobalSegmentId, NodeId>>
  node_keys:      Vec<Vec<KeyId>>              leaf keys ending at this node
  node_bundle:    Vec<BundleId>

Keys
  key_node:       Vec<NodeId>                  leaf node for this key
  key_bundle:     Vec<BundleId>

Translations
  translations:   HashMap<(KeyId, GlobalLocaleId), String>

Exception tables  (drive filter visibility and display)
  dirty:          HashSet<KeyId>
  pinned:         HashSet<KeyId>
  temp_pins:      HashSet<KeyId>

Cross-bundle index
  seg_nodes:      HashMap<GlobalSegmentId, Vec<NodeId>>  all nodes for a segment
```

The trie per bundle has a **virtual root node** with `node_segment = None` and
`node_parent = None`. All real key-path nodes descend from it.

---

## Building the store

```rust
let store = Store::from_workspace(&workspace);
```

Iterates all file groups in the workspace, calls `register_locale` for each
locale file, then `insert_key` + `set_translation` for each key-value entry.
`insert_key` is idempotent — the same key appearing in multiple locale files
is safe; the second call returns the existing `KeyId`.

After this call the workspace is only needed for write-back (flushing pending
changes to disk).

---

## Query API

### String resolution

```rust
store.segment_str(seg_id)      -> &str
store.locale_str(locale_id)    -> &str
store.bundle_name_str(bundle_id) -> &str
store.key_real_key_str(key_id) -> String   // dot-joined, no bundle prefix
store.key_qualified_str(key_id) -> String  // "bundle:real.key"
```

### Enumeration

```rust
// All bundles in insertion order
store.bundle_ids() -> impl Iterator<Item = BundleId>

// All keys in a bundle, in insertion order
store.bundle_keys(bundle_id) -> impl Iterator<Item = KeyId>

// All locales registered for a bundle
store.bundle_locales(bundle_id) -> &[GlobalLocaleId]

// All nodes carrying a given segment (cross-bundle)
store.nodes_for_segment(seg_id) -> &[NodeId]
```

### Structural accessors

These return references to internal data — slices or maps — giving callers
full capability (`.len()`, `.get()`, `.iter()`) without opaque wrappers:

```rust
store.bundle_root(bundle_id)         -> NodeId
store.node_segment(node)             -> Option<GlobalSegmentId>
store.node_parent(node)              -> Option<NodeId>
store.node_children(node)            -> &HashMap<GlobalSegmentId, NodeId>
store.node_keys(node)                -> &[KeyId]
store.node_bundle(node)              -> BundleId
store.key_node(key_id)               -> NodeId
store.key_bundle(key_id)             -> BundleId
store.key_segments(key_id)           -> Vec<GlobalSegmentId>  // root → leaf
```

### Translation lookup

```rust
store.translation(key_id, locale_id) -> Option<&str>
// None = key absent in this locale file
```

### Chain-collapse queries

These are the high-level structural queries used by the rendering layer.

**`key_compressed(key_id) -> Vec<Vec<GlobalSegmentId>>`**

Returns the full root-to-leaf path for a key, split into **partition groups**
by the chain-collapse rule. A run of nodes where each node has exactly one
child and no key is collapsed into a single group. Branch points (≥2 children)
and key nodes end a group.

```
keys: app.dialog.yes, app.dialog.no

Trie structure:
  root → app (1 child) → dialog (2 children) → yes (leaf key)
                                              → no  (leaf key)

key_compressed(yes) = [[app, dialog], [yes]]
key_compressed(no)  = [[app, dialog], [no]]
  ^                    ^^^^^^^^^^^^^  ^^^
  groups               collapsed      branch ends group
```

```
keys: app.title  (single key, no siblings)

key_compressed(title) = [[app, title]]
  whole path collapses into one group — nothing to split on
```

**`node_display_chain(node) -> Vec<GlobalSegmentId>`**

The segments to display on the row anchored at `node` — only the segments
since the last visible branch, not the full root-to-leaf path. Walks from
`node` toward the root, climbing while the parent is chain-collapsible (one
child, no key, has a segment).

```
Same trie as above:

node_display_chain(dialog_node) = [app, dialog]   ← the header row shows both
node_display_chain(yes_node)    = [yes]            ← the leaf row shows only "yes"
```

---

## Example: building a visible row list

This is what `DomainModel::visible_rows` does internally:

```rust
// For each bundle (alphabetical):
for bundle_id in sorted_bundle_ids {
    emit bundle-header row (partitions: vec![], indent: 0)

    let mut visible_keys = store.bundle_keys(bundle_id)
        .filter(|&k| key_visible(k))          // apply filter predicate
        .collect::<Vec<_>>();
    visible_keys.sort_by_key(|&k| store.key_real_key_str(k));

    let mut prev_groups: Vec<Vec<GlobalSegmentId>> = vec![];
    for key_id in visible_keys {
        let groups  = store.key_compressed(key_id);
        let shared  = count_shared_prefix(&prev_groups, &groups);
        let leaf_gi = groups.len() - 1;

        // emit a group-header row for every new partition group except the last
        for gi in shared..leaf_gi {
            emit header row (partitions: groups[gi].clone(), indent: gi)
        }

        // emit the leaf row
        emit key row (partitions: groups[leaf_gi].clone(), indent: leaf_gi,
                      key_id: Some(key_id),
                      has_children: !store.node_children(store.key_node(key_id)).is_empty())

        prev_groups = groups;
    }
}
```

Header suppression is free: a header is only emitted when a visible key needs
it. No placeholder push/pop required.

---

## Example: filter predicate composition

The `key_visible` predicate can encode any combination of filter rules without
the store knowing about them:

```rust
// Show all keys (no filter)
dm.visible_rows(|_| true)

// Only keys in bundle "messages"
let msg_id = /* find bundle id */;
dm.visible_rows(|k| store.key_bundle(k) == msg_id)

// Only dirty keys
dm.visible_rows(|k| store.dirty.contains(&k))

// Key substring match + not deleted (future soft-delete)
dm.visible_rows(|k| {
    !store.deleted.contains(&k)
    && store.key_real_key_str(k).contains("confirm")
})

// Pinned keys always visible regardless of other filters
dm.visible_rows(|k| {
    store.pinned.contains(&k) || passes_filter(k)
})
```

---

## Write operations (current)

### Register a locale file

```rust
store.register_locale("messages", "de");
// Creates the bundle "messages" if it doesn't exist.
// Registers locale "de" for it. Safe to call multiple times — idempotent.
```

### Insert a key

```rust
let key_id = store.insert_key("messages", "app.dialog.title");
// Walks / creates trie nodes for each segment.
// Returns existing KeyId if the key already exists — idempotent.
// Safe to call once per locale file for the same key.
```

### Set / update a translation

```rust
store.set_translation(key_id, "de", "Titel".to_string());
// Overwrites any existing value for this (key, locale) pair.
```

### Remove a translation

```rust
store.remove_translation(key_id, "de");
// Removes the cell. The key remains in the trie; other locales are unaffected.
```

---

## Planned mutation model

These are designed and documented here for implementation guidance. Not yet
in the code.

### Typed key mutation state

Replace the single `dirty: HashSet<KeyId>` with a map that tracks what kind
of change each key has:

```rust
pub enum KeyMutation {
    Inserted,   // new key, no file backing yet        → render green
    Deleted,    // marked for deletion                 → render red
    Modified,   // existing key with changed values    → render yellow
}

pub mutations: HashMap<KeyId, KeyMutation>
```

`is_dirty(key_id)` = `mutations.contains_key(&key_id)`.

State machine for transitions:
- Insert then delete in same session → remove from map entirely (no write needed)
- Modify then delete → `Deleted` wins; drop the pending `Update` writes

### Soft delete

```rust
pub deleted: HashSet<KeyId>
```

Keys in `deleted` stay in the trie — no structural changes. The `key_visible`
predicate hides them by default; a filter term (e.g. `/*-`) surfaces them.

- **Delete:** add to `deleted`, queue `PendingChange::Delete`
- **Undo:** remove from `deleted`, remove the queued `PendingChange::Delete`
- **Save:** flush delete writes, clear from `deleted`

No trie pruning needed. Deleted nodes in the trie have no effect on
`key_compressed` for other keys because the deleted keys simply don't pass
the visibility predicate.

### Subtree / segment rename

Renaming one segment (e.g. `dialog` → `modal`) renames ALL descendant keys
simultaneously — it is a single node update in the trie:

```rust
// in-memory: O(1)
self.node_segment[node.idx()] = new_segment_id;

// tracking:
pub segment_renames: HashMap<NodeId, GlobalSegmentId>
//                           ^node    ^old segment id
```

Queries:

```rust
// Is this key's path affected by any segment rename?
fn key_is_renamed(&self, key_id: KeyId) -> bool {
    self.key_segments(key_id)          // walk root → leaf
        .iter()
        .any(|&seg| /* node for this seg is in segment_renames */)
}

// Reconstruct the pre-rename qualified key string for display
fn key_old_path(&self, key_id: KeyId) -> String { ... }
```

- **Undo:** restore `node_segment[node.idx()]` from the map, remove entry
- **Save:** generate `PendingChange::Rename` for each affected key in each
  locale file — this is O(n) but only at write time, not at mutation time

Granularity is free: rename a leaf segment (single key), an internal node
(whole subtree), or anything in between — same mechanism.

### Cell-level mutation colors

Per-cell color is derivable from the `PendingChange` variant — no new store
state needed:

```
PendingChange::Update { .. }  →  yellow cell (value changed)
PendingChange::Insert { .. }  →  green cell  (locale didn't have this key)
PendingChange::Delete { .. }  →  red cell    (locale cell removed)
```

The renderer reads `pending_writes` at draw time to determine per-cell color.

---

## Filter DSL integration (planned)

When the filter predicate is built from a `FilterExpr`, it operates on `KeyId`s
rather than strings. Mapping filter terms to store queries:

| Filter term     | Store query                                          |
|-----------------|------------------------------------------------------|
| `bundle:name`   | `store.key_bundle(k) == bundle_id`                  |
| `/pattern`      | `store.key_real_key_str(k).contains(pattern)`        |
| `/#`            | `store.mutations.contains_key(&k)`                   |
| `/*+` (planned) | `store.mutations.get(&k) == Some(Inserted)`          |
| `/*-` (planned) | `store.deleted.contains(&k)`                         |
| `:de?`          | locale cell is missing for `de`                      |
| pinned bypass   | `store.pinned.contains(&k) \|\| predicate(k)`        |

All composition (AND, OR, NOT) happens in the predicate closure — the store
stays unaware of filter expressions.
