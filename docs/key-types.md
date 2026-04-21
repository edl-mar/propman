# Key Type Hierarchy

## Why

Keys are currently represented as raw strings throughout the codebase:
`"bundle:seg1.seg2.seg3"` is the lingua franca passed between the workspace,
navigation helpers, filter, renderer, and operations layer.  This works but
has real costs:

- **String parsing everywhere.**  `prefix.find(':')`, `key.split('.')`,
  `key.rfind('.')` appear across `state.rs`, `app_model.rs`, `view_model.rs`,
  and the ops modules.  The same parsing logic is repeated and easy to get
  wrong at the edges (bare keys with no bundle, empty segments, the colon as
  separator vs. part of a value).

- **No structure, no invariants.**  A `String` cannot express "this path was
  validated by the trie" or "this path has children" or "these segments form
  two chain-collapsed visual nodes".  Callers check `is_leaf`, `has_children`,
  and branch/chain status by re-querying the trie or by reading flags that have
  drifted from the trie's actual state.

- **`common_prefix`, `parent`, `push` are free functions or ad-hoc.**
  `prefix_at_depth`, `parent_prefix`, `prefix_depth` in `state.rs` are
  string-level reimplementations of operations that belong on a key type.
  `common_prefix_len` in `app_model.rs` works on `&[String]` slices because
  there is no type that carries segments pre-split.

- **The partition structure leaks into the renderer.**  `build_view_rows` calls
  `trie.key_partitions()`, gets back `Vec<Range<usize>>`, and manually maps
  those ranges into display slices.  The renderer has to know about trie
  internals that should be encapsulated behind a type.

The goal of the key type hierarchy is to fix all of this: one clean set of
types, structural operations as methods, and trie knowledge expressed in the
type rather than re-derived on every render pass.

## Type Hierarchy

```
KeySegment          one dot-free segment — a wrapper around String
KeyPartition        one or more KeySegments forming one visual/trie node
KeyData             the segment data of a Key, partitioned or not
ResolvedData        trie-derived structural facts about a Key
Key                 the complete key type
```

### `KeySegment`

```rust
pub struct KeySegment(String);
```

The atomic unit.  One segment of a dot-separated key path — `"app"`,
`"confirm"`, `"title"`.  A wrapper so that segment lists are typed and cannot
be confused with arbitrary string lists.

### `KeyPartition`

```rust
pub struct KeyPartition(Vec<KeySegment>);
```

One contiguous group of segments that forms a single visual node.

- **Single-segment partition**: the common case — one trie branch level.
- **Multi-segment partition**: a chain-collapsed run of single-child nodes,
  e.g. `["detail", "something"]` rendered as `detail.something:` in one header
  row.  The trie computes these; you cannot know a chain exists without it.

### `KeyData`

```rust
pub enum KeyData {
    /// Segments without explicit partition structure.
    /// Iteration treats each segment as its own single-element partition.
    Unpartitioned(Vec<KeySegment>),
    /// Segments with explicit partition structure, computed by the trie.
    /// Each partition may span multiple segments (chain-collapsed nodes).
    Partitioned(Vec<KeyPartition>),
}
```

`Unpartitioned` is what you get when you construct a key freely from a string
or from segment manipulation (`push`, `parent`, `common_prefix`,
`split_last`).  The `partitions()` method on `KeyData` still works — it wraps
each segment as a single-element partition — so callers iterate uniformly
regardless of which variant they hold.

`Partitioned` is what the trie produces when it resolves a key.  The partition
structure records exactly how the renderer should group segments into header
rows and leaf rows, without the renderer needing to call back into the trie.

### `ResolvedData`

```rust
pub struct ResolvedData {
    pub is_leaf:     bool,   // this path has a stored value
    pub child_count: usize,  // number of direct child nodes in the trie
}
```

Structural facts that only the trie can know.  Stored on a `Key` when the
trie was consulted.

`child_count` subsumes the common derived questions:

```rust
resolved.child_count > 0   // has any children
resolved.child_count > 1   // is a branch — callers use this for chain-collapse logic
resolved.child_count == 1  // single-child chain candidate
```

The "branch" concept is intentionally not named here — it belongs to the
renderer's decision of how to display the key, not to the key itself.

A key where `is_leaf: true` and `child_count > 0` is a **key-and-parent
node** — a translatable entry that also has children.  This is a real case
(e.g. `com.myapp.error` exists as a key while `com.myapp.error.notfound` also
exists).

### `Key`

```rust
pub struct Key {
    pub bundle:   Option<String>,       // None = bare/legacy key space
    pub data:     KeyData,
    pub is_prefix: bool,                // constructed as a prefix path
    pub resolved:  Option<ResolvedData>, // None = not trie-confirmed
}
```

`bundle: None` is the bare/legacy key space (keys with no bundle prefix).
`bundle: Some("messages")` with an empty segment list is a bundle root
position — no separate variant is needed.

`is_prefix` is a construction-time flag: this key was produced by
`parent()`, `common_prefix()`, or `split_last()`, and may not reach a leaf.
It is distinct from `resolved.child_count > 0` — the latter is the trie's
authoritative answer; `is_prefix` records intent at construction time for
unresolved keys.

`resolved: None` means the key has not been confirmed against the trie.
`resolved: Some(...)` means the trie was consulted and the structural facts
are accurate.

## Combinations

| `data` | `resolved` | meaning |
|---|---|---|
| `Unpartitioned` | `None` | freely constructed, unvalidated |
| `Unpartitioned` | `Some` | trie confirmed existence; partition structure not requested |
| `Partitioned` | `None` | manually partitioned, not yet trie-confirmed |
| `Partitioned` | `Some` | trie produced this — the normal fully-resolved case |

## Key Operations

All structural navigation returns `Key` with `Unpartitioned` data and inherits
`bundle`.  Navigation discards partition structure because stepping through the
hierarchy produces a position that the trie has not re-validated:

```
key.parent()          → Key (is_prefix: true,  resolved: None)
key.push(seg)         → Key (is_prefix: false, resolved: None)
key.split_last()      → (Key as prefix, KeySegment)
key.common_prefix(b)  → Key (is_prefix: true,  resolved: None)
key.segments()        → impl Iterator<Item = &KeySegment>  (flattens partitions)
key.partitions()      → impl Iterator<Item = &[KeySegment]>
key.depth()           → usize  (total segment count across all partitions)
```

## Trie API

The trie is the only constructor for `Partitioned` data and for
`resolved: Some(...)`.  Its methods return `Key` values directly:

```rust
impl BundleModel {
    /// Resolve a key: confirm it exists and populate partition structure and
    /// structural facts.  Returns None when the path is not in the trie.
    pub fn resolve(&self, key: &Key) -> Option<Key>;

    /// Resolve all partitions for a full leaf path: returns one Key per
    /// visual node (headers + leaf), each with ResolvedData.
    /// Replaces the current key_partitions() → Vec<Range<usize>> API.
    pub fn resolve_partitions(&self, key: &Key) -> Vec<Key>;
}
```

## What This Replaces

| Current | Replaced by |
|---|---|
| `fn prefix_depth(prefix: &str)` | `key.depth()` |
| `fn prefix_at_depth(prefix, depth)` | `key.parent()` repeated / `key.truncate(depth)` |
| `fn parent_prefix(prefix: &str)` | `key.parent()` |
| `fn qualify(bundle, key)` | `Key { bundle: Some(...), .. }` construction |
| `fn common_prefix_len(a, b)` | `key.common_prefix(other)` |
| `trie.key_partitions() → Vec<Range<usize>>` | `bundle.resolve_partitions(key)` |
| `RowIdentity.prefix: String` | `RowIdentity.prefix: Key` |
| `RowIdentity.full_key: Option<String>` | `RowIdentity.full_key: Option<Key>` |
| `HashSet<String>` for dirty/pinned | boundary: `key.to_qualified_string()` at workspace edges |
