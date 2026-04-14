// ── safety contract ───────────────────────────────────────────────────────────
//
// CompressedTrie owns a bumpalo::Bump arena. All string buffers and segment
// slices are allocated into that arena. Nodes store raw pointers (*const str,
// *const [KeySegment]) instead of references, so no lifetime parameter is
// needed on any type.
//
// Safety is upheld by declaration order: `root` is declared before `arena` in
// CompressedTrie. Rust drops fields in declaration order, so `root` (and all
// nodes reachable from it) are dropped before `arena`. The raw pointers inside
// nodes are therefore never dereferenced after the arena is freed.
//
// The public API is entirely safe. All unsafe is confined to the two
// dereference sites in KeyPartition::as_slice() and KeySegment::as_str().
//
// Reloading: drop the old CompressedTrie (frees arena + all nodes), create a
// new one. No leaks, no partial cleanup needed.

use bumpalo::Bump;
use std::collections::BTreeMap;

// ── key types ─────────────────────────────────────────────────────────────────

/// A single dot-separated segment.
/// Stores a raw pointer into the arena — no lifetime, freely copyable.
///
/// Equality, ordering, and hashing are all by string content so that segments
/// from different `parse_key` calls compare correctly in the children map.
#[derive(Clone, Copy)]
pub struct KeySegment {
    ptr: *const u8,
    len: usize,
}

// KeySegment contains a raw pointer, which makes it !Send + !Sync by default.
// SAFETY: the pointer is immutable and only valid while the arena lives.
// Since CompressedTrie is the sole owner of both the arena and all KeySegments,
// and we never hand out KeySegments that outlive the trie, this is safe.
unsafe impl Send for KeySegment {}
unsafe impl Sync for KeySegment {}

impl KeySegment {
    fn from_str(s: &str) -> Self {
        KeySegment {
            ptr: s.as_ptr(),
            len: s.len(),
        }
    }

    /// SAFETY: caller must ensure the arena that allocated this segment is alive.
    pub fn as_str(&self) -> &str {
        unsafe {
            let slice = std::slice::from_raw_parts(self.ptr, self.len);
            std::str::from_utf8_unchecked(slice)
        }
    }
}

impl std::fmt::Debug for KeySegment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "KeySegment({:?})", self.as_str())
    }
}

impl PartialEq for KeySegment {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}
impl Eq for KeySegment {}

impl std::hash::Hash for KeySegment {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl PartialOrd for KeySegment {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for KeySegment {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

// Allows BTreeMap::get("segment") without allocating a KeySegment.
impl std::borrow::Borrow<str> for KeySegment {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

/// A lightweight, copyable view into a contiguous range of segments in the arena.
/// Just three words: raw pointer + start + end. No lifetime, no allocation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct KeyPartition {
    segments: *const KeySegment,
    len: usize, // total length of the slice
    start: usize,
    end: usize,
}

unsafe impl Send for KeyPartition {}
unsafe impl Sync for KeyPartition {}

impl KeyPartition {
    pub fn len(self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }

    pub fn first(self) -> KeySegment {
        self.as_slice()[0]
    }

    /// SAFETY: caller must ensure the arena is alive.
    pub fn as_slice(&self) -> &[KeySegment] {
        unsafe {
            let base = std::slice::from_raw_parts(self.segments, self.len);
            &base[self.start..self.end]
        }
    }

    pub fn slice(self, start: usize, end: usize) -> KeyPartition {
        assert!(start <= end && end <= self.len());
        KeyPartition {
            segments: self.segments,
            len: self.len,
            start: self.start + start,
            end: self.start + end,
        }
    }

    pub fn tail(self, n: usize) -> KeyPartition {
        self.slice(n, self.len())
    }

    pub fn head(self, n: usize) -> KeyPartition {
        self.slice(0, n)
    }

    pub fn common_prefix_len(self, other: KeyPartition) -> usize {
        self.as_slice()
            .iter()
            .zip(other.as_slice().iter())
            .take_while(|(a, b)| a == b)
            .count()
    }
}

impl std::fmt::Debug for KeyPartition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let segs: Vec<&str> = self.as_slice().iter().map(|s| s.as_str()).collect();
        write!(f, "KeyPartition({:?})", segs)
    }
}

/// A fully parsed key — just two raw-pointer handles into the arena.
/// Copy and cheap to pass around.
#[derive(Clone, Copy)]
pub struct PartitionedKey {
    raw: *const u8, // pointer to the original key string in the arena
    raw_len: usize,
    partition: KeyPartition,
}

unsafe impl Send for PartitionedKey {}
unsafe impl Sync for PartitionedKey {}

impl PartitionedKey {
    pub fn partition(self) -> KeyPartition {
        self.partition
    }

    pub fn slice(self, start: usize, end: usize) -> KeyPartition {
        self.partition.slice(start, end)
    }

    pub fn raw(&self) -> &str {
        unsafe {
            let slice = std::slice::from_raw_parts(self.raw, self.raw_len);
            std::str::from_utf8_unchecked(slice)
        }
    }
}

impl std::fmt::Debug for PartitionedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PartitionedKey({:?})", self.raw())
    }
}

// ── node types ────────────────────────────────────────────────────────────────

/// Key terminates here, no children.
#[derive(Debug)]
pub struct Leaf<V> {
    pub value: V,
    pub deleted: bool,
}

/// Pure branch point — no key terminates here.
#[derive(Debug)]
pub struct Branch<V> {
    pub children: BTreeMap<KeySegment, (KeyPartition, Node<V>)>,
}

/// Key terminates here AND has children.
/// e.g. `http.status` is itself a key and a prefix of `http.status.200`.
#[derive(Debug)]
pub struct Interior<V> {
    pub value: V,
    pub deleted: bool,
    pub children: BTreeMap<KeySegment, (KeyPartition, Node<V>)>,
}

#[derive(Debug)]
pub enum Node<V> {
    Leaf(Leaf<V>),
    Branch(Branch<V>),
    Interior(Interior<V>),
}

impl<V> Node<V> {
    pub fn child_count(&self) -> usize {
        match self {
            Node::Leaf(_) => 0,
            Node::Branch(b) => b.children.len(),
            Node::Interior(i) => i.children.len(),
        }
    }

    /// Children in alphabetical order (BTreeMap guarantees sorted iteration).
    /// Each entry is `(edge: KeyPartition, child: &Node<V>)`.
    pub fn children(&self) -> Vec<(KeyPartition, &Node<V>)> {
        match self {
            Node::Leaf(_) => vec![],
            Node::Branch(b) => b.children.values().map(|(p, n)| (*p, n)).collect(),
            Node::Interior(i) => i.children.values().map(|(p, n)| (*p, n)).collect(),
        }
    }
}

// ── trie ──────────────────────────────────────────────────────────────────────

/// A compressed radix tree that owns its arena.
///
/// Field declaration order is load-bearing:
///   1. `root`  — dropped first; all nodes and their raw pointers go away
///   2. `arena` — dropped second; frees all string/segment memory safely
///
/// Reloading is simply dropping this struct and creating a new one.
pub struct CompressedTrie<V> {
    root: Option<Node<V>>, // ← dropped first
    arena: Bump,           // ← dropped second
}

impl<V> std::fmt::Debug for CompressedTrie<V>
where
    V: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompressedTrie")
            .field("root", &self.root)
            .finish()
    }
}

impl<V> CompressedTrie<V> {
    pub fn new() -> Self {
        Self {
            root: None,
            arena: Bump::new(),
        }
    }

    /// Parse a key string into a PartitionedKey allocated in the trie's arena.
    /// The returned PartitionedKey is valid for the lifetime of this trie.
    pub fn parse_key(&self, key: &str) -> PartitionedKey {
        // Allocate the raw string into the arena.
        let raw_arena: &str = self.arena.alloc_str(key);
        let raw = raw_arena.as_ptr();
        let raw_len = raw_arena.len();

        // Build segments referencing the arena string, then allocate the slice.
        let segs: Vec<KeySegment> = raw_arena.split('.').map(KeySegment::from_str).collect();
        let seg_slice: &[KeySegment] = self.arena.alloc_slice_copy(&segs);

        PartitionedKey {
            raw,
            raw_len,
            partition: KeyPartition {
                segments: seg_slice.as_ptr(),
                len: seg_slice.len(),
                start: 0,
                end: seg_slice.len(),
            },
        }
    }

    /// Insert using a pre-parsed KeyPartition.
    pub fn insert(&mut self, key: KeyPartition, value: V) -> Option<V> {
        // Use an empty Branch as the implicit root so the first inserted key
        // gets a proper edge rather than being stored at the empty prefix.
        let root = self.root.take()
            .unwrap_or(Node::Branch(Branch { children: BTreeMap::new() }));
        let (new_root, old) = insert_node(root, key, value);
        self.root = Some(new_root);
        old
    }

    /// Parse and insert in one step.
    pub fn insert_str(&mut self, key: &str, value: V) -> Option<V> {
        let k = self.parse_key(key);
        self.insert(k.partition(), value)
    }

    /// Get using a pre-parsed KeyPartition.
    pub fn get(&self, key: KeyPartition) -> Option<&V> {
        self.root.as_ref().and_then(|n| get_node(n, key))
    }

    /// Parse and get in one step.
    pub fn get_str(&self, key: &str) -> Option<&V> {
        let k = self.parse_key(key);
        self.get(k.partition())
    }

    /// Soft-delete — marks entry as logically absent without structural removal.
    /// Returns true if the key existed and was not already deleted.
    pub fn delete(&mut self, key: KeyPartition) -> bool {
        self.root
            .as_mut()
            .map(|n| soft_delete_node(n, key))
            .unwrap_or(false)
    }

    /// Restore a soft-deleted entry.
    /// Returns true if the key existed and was deleted.
    pub fn restore(&mut self, key: KeyPartition) -> bool {
        self.root
            .as_mut()
            .map(|n| restore_node(n, key))
            .unwrap_or(false)
    }

    pub fn contains(&self, key: KeyPartition) -> bool {
        self.get(key).is_some()
    }

    // ── structural query API ──────────────────────────────────────────────────

    /// Return the node at `path`, if one exists.
    ///
    /// Unlike `get`, this also returns `Branch` nodes (structural nodes with no
    /// value).  Used for display queries: "is there a header at this prefix?",
    /// "does this key have children?".
    pub fn node_at(&self, path: &[&str]) -> Option<&Node<V>> {
        self.root.as_ref().and_then(|n| find_node(n, path))
    }

    /// True when the node at `path` has ≥ 2 children — i.e. it should render
    /// as a group-header row in the key column.
    pub fn is_branch_at(&self, path: &[&str]) -> bool {
        self.node_at(path).map_or(false, |n| n.child_count() >= 2)
    }

    /// True when the node at `path` has any children.
    /// Used to detect key-and-parent entries (Interior nodes).
    pub fn has_children_at(&self, path: &[&str]) -> bool {
        self.node_at(path).map_or(false, |n| n.child_count() > 0)
    }

    /// Children of the node at `path`, in alphabetical order.
    /// Each item is `(edge: KeyPartition, child: &Node<V>)`.
    /// The edge spans one or more segments (multi-segment = chain-collapsed).
    pub fn children_of(&self, path: &[&str]) -> Vec<(KeyPartition, &Node<V>)> {
        self.node_at(path).map_or_else(Vec::new, |n| n.children())
    }

    /// True when `path` is a rendering partition boundary.
    ///
    /// A boundary exists when:
    /// - The node is a `Branch` with ≥2 children (pure structural branch → header row), or
    /// - The node is an `Interior` (key-and-parent → children must be at a deeper visual depth).
    ///
    /// Used by `build_partitioned_key` to determine where to split the key segment list
    /// into partitions.  `is_branch_at` is the subset that also emits a header visual row;
    /// `is_render_boundary_at` additionally covers single-child key-and-parent nodes.
    pub fn is_render_boundary_at(&self, path: &[&str]) -> bool {
        match self.node_at(path) {
            Some(Node::Branch(b)) => b.children.len() >= 2,
            Some(Node::Interior(_)) => true,
            _ => false,
        }
    }

    /// Split a key's segments into render partitions.
    ///
    /// Returns a non-empty list of half-open index ranges into `segs`.
    /// Each range except the last ends at a render boundary (a node with ≥2
    /// children, or a key-node that also has children).  The final range covers
    /// the remaining segments down to the leaf.
    ///
    /// Single-child chains that are compressed away in the trie produce no
    /// boundary, so their segments end up in one range together.
    ///
    /// Examples:
    ///   `["app","confirm","delete"]` with boundaries at `app` and `app.confirm`
    ///     → `[0..1, 1..2, 2..3]`
    ///   `["com","myapp","error","notfound"]` compressed to one boundary at depth 2
    ///     → `[0..3, 3..4]`
    ///   `["loading"]` with no boundaries
    ///     → `[0..1]`
    pub fn key_partitions(&self, segs: &[&str]) -> Vec<std::ops::Range<usize>> {
        let mut partitions = Vec::new();
        let mut prev_end = 0;
        for d in 0..segs.len().saturating_sub(1) {
            if self.is_render_boundary_at(&segs[..=d]) {
                partitions.push(prev_end..d + 1);
                prev_end = d + 1;
            }
        }
        partitions.push(prev_end..segs.len());
        partitions
    }
}

impl<V> Default for CompressedTrie<V> {
    fn default() -> Self {
        Self::new()
    }
}

// ── node helpers ──────────────────────────────────────────────────────────────

fn into_parts<V>(
    node: Node<V>,
) -> (
    Option<(V, bool)>,
    BTreeMap<KeySegment, (KeyPartition, Node<V>)>,
) {
    match node {
        Node::Leaf(l) => (Some((l.value, l.deleted)), BTreeMap::new()),
        Node::Branch(b) => (None, b.children),
        Node::Interior(i) => (Some((i.value, i.deleted)), i.children),
    }
}

fn make_node<V>(
    entry: Option<(V, bool)>,
    children: BTreeMap<KeySegment, (KeyPartition, Node<V>)>,
) -> Node<V> {
    match (entry, children.is_empty()) {
        (Some((value, deleted)), true) => Node::Leaf(Leaf { value, deleted }),
        (Some((value, deleted)), false) => Node::Interior(Interior {
            value,
            deleted,
            children,
        }),
        (None, false) => Node::Branch(Branch { children }),
        (None, true) => panic!("make_node: empty node — illegal trie state"),
    }
}

// ── structural traversal ─────────────────────────────────────────────────────

/// Navigate to the node at `path` without requiring the path to be
/// arena-allocated.  Returns `Some(&node)` for any node type including
/// `Branch` (unlike `get_node` which returns `None` for valueless nodes).
fn find_node<'a, V>(node: &'a Node<V>, path: &[&str]) -> Option<&'a Node<V>> {
    if path.is_empty() {
        return Some(node);
    }
    let children = match node {
        Node::Branch(b) => &b.children,
        Node::Interior(i) => &i.children,
        Node::Leaf(_) => return None,
    };
    let (edge, child) = children.get(path[0])?;
    let edge_segs = edge.as_slice();
    // The full edge must be a prefix of (or equal to) `path`.
    if path.len() < edge_segs.len() {
        return None;
    }
    if !edge_segs.iter().zip(path.iter()).all(|(e, p)| e.as_str() == *p) {
        return None;
    }
    find_node(child, &path[edge_segs.len()..])
}

// ── core recursive operations ─────────────────────────────────────────────────
//
// KeyPartition is Copy — passed by value throughout, zero cloning.

fn insert_node<V>(node: Node<V>, key: KeyPartition, value: V) -> (Node<V>, Option<V>) {
    if key.is_empty() {
        return match node {
            Node::Leaf(l) => {
                let old = if !l.deleted { Some(l.value) } else { None };
                (
                    Node::Leaf(Leaf {
                        value,
                        deleted: false,
                    }),
                    old,
                )
            }
            Node::Branch(b) => (
                Node::Interior(Interior {
                    value,
                    deleted: false,
                    children: b.children,
                }),
                None,
            ),
            Node::Interior(i) => {
                let old = if !i.deleted { Some(i.value) } else { None };
                (
                    Node::Interior(Interior {
                        value,
                        deleted: false,
                        children: i.children,
                    }),
                    old,
                )
            }
        };
    }

    let (entry, mut children) = into_parts(node);
    let first = key.first();

    let old = if let Some((edge, child)) = children.remove(&first) {
        let cp = edge.common_prefix_len(key);

        if cp == edge.len() {
            let (new_child, old) = insert_node(child, key.tail(cp), value);
            children.insert(first, (edge, new_child));
            old
        } else {
            // Partial match — split the edge at the common prefix.
            //
            //  Before:  node --[edge]--> child
            //  After:   node --[head]--> split
            //                            ├─[edge.tail(cp)]--> child  (old)
            //                            └─[key.tail(cp)] --> leaf   (new)
            let head = edge.head(cp);
            let edge_tail = edge.tail(cp);
            let key_tail = key.tail(cp);

            let mut split_children: BTreeMap<KeySegment, (KeyPartition, Node<V>)> = BTreeMap::new();
            split_children.insert(edge_tail.first(), (edge_tail, child));

            let split_node = if key_tail.is_empty() {
                Node::Interior(Interior {
                    value,
                    deleted: false,
                    children: split_children,
                })
            } else {
                let leaf = Node::Leaf(Leaf {
                    value,
                    deleted: false,
                });
                split_children.insert(key_tail.first(), (key_tail, leaf));
                Node::Branch(Branch {
                    children: split_children,
                })
            };

            children.insert(head.first(), (head, split_node));
            None
        }
    } else {
        children.insert(
            first,
            (
                key,
                Node::Leaf(Leaf {
                    value,
                    deleted: false,
                }),
            ),
        );
        None
    };

    (make_node(entry, children), old)
}

fn get_node<V>(node: &Node<V>, key: KeyPartition) -> Option<&V> {
    if key.is_empty() {
        return match node {
            Node::Leaf(l) if !l.deleted => Some(&l.value),
            Node::Interior(i) if !i.deleted => Some(&i.value),
            _ => None,
        };
    }

    let children = match node {
        Node::Branch(b) => &b.children,
        Node::Interior(i) => &i.children,
        Node::Leaf(_) => return None,
    };

    let (edge, child) = children.get(&key.first())?;
    let cp = edge.common_prefix_len(key);
    if cp < edge.len() {
        return None;
    }
    get_node(child, key.tail(cp))
}

fn soft_delete_node<V>(node: &mut Node<V>, key: KeyPartition) -> bool {
    if key.is_empty() {
        return match node {
            Node::Leaf(l) if !l.deleted => {
                l.deleted = true;
                true
            }
            Node::Interior(i) if !i.deleted => {
                i.deleted = true;
                true
            }
            _ => false,
        };
    }

    let children = match node {
        Node::Branch(b) => &mut b.children,
        Node::Interior(i) => &mut i.children,
        Node::Leaf(_) => return false,
    };

    let Some((edge, child)) = children.get_mut(key.first().as_str()) else { return false; };
    let cp = edge.common_prefix_len(key);
    if cp < edge.len() {
        return false;
    }
    soft_delete_node(child, key.tail(cp))
}

fn restore_node<V>(node: &mut Node<V>, key: KeyPartition) -> bool {
    if key.is_empty() {
        return match node {
            Node::Leaf(l) if l.deleted => {
                l.deleted = false;
                true
            }
            Node::Interior(i) if i.deleted => {
                i.deleted = false;
                true
            }
            _ => false,
        };
    }

    let children = match node {
        Node::Branch(b) => &mut b.children,
        Node::Interior(i) => &mut i.children,
        Node::Leaf(_) => return false,
    };

    let Some((edge, child)) = children.get_mut(key.first().as_str()) else { return false; };
    let cp = edge.common_prefix_len(key);
    if cp < edge.len() {
        return false;
    }
    restore_node(child, key.tail(cp))
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn populated() -> (CompressedTrie<usize>, Vec<PartitionedKey>) {
        let raw_keys = [
            "http.something",
            "http.status",
            "http.status.200",
            "http.status.400",
            "http.status.401",
            "http.status.403",
            "http.status.404",
            "http.status.500",
            "http.status.detail.something.msg.firstmessage",
            "http.status.detail.something.msg.secondmessage",
            "http.status.detail.something.notmsg",
            "http.status.x",
            "http.y",
        ];
        let mut t = CompressedTrie::new();
        // Parse once into arena, reuse the PartitionedKey handles everywhere.
        let keys: Vec<PartitionedKey> = raw_keys.iter().map(|k| t.parse_key(k)).collect();
        for (i, k) in keys.iter().enumerate() {
            t.insert(k.partition(), i);
        }
        (t, keys)
    }

    #[test]
    fn inserts_and_gets() {
        let (t, keys) = populated();
        assert_eq!(t.get(keys[1].partition()), Some(&1));
        assert_eq!(t.get(keys[6].partition()), Some(&6));
        assert_eq!(t.get(keys[8].partition()), Some(&8));
        assert_eq!(t.get(keys[12].partition()), Some(&12));
    }

    #[test]
    fn missing_keys_return_none() {
        let (t, _) = populated();
        assert_eq!(t.get_str("http"), None);
        assert_eq!(t.get_str("http.status.999"), None);
        assert_eq!(t.get_str("http.status.40"), None);
        assert_eq!(t.get_str("http.status.detail"), None);
    }

    #[test]
    fn overwrite_returns_old_value() {
        let (mut t, keys) = populated();
        let old = t.insert(keys[6].partition(), 99);
        assert_eq!(old, Some(6));
        assert_eq!(t.get(keys[6].partition()), Some(&99));
    }

    #[test]
    fn soft_delete_hides_value() {
        let (mut t, keys) = populated();
        assert!(t.delete(keys[6].partition()));
        assert_eq!(t.get(keys[6].partition()), None);
        assert_eq!(t.get(keys[5].partition()), Some(&5));
        assert_eq!(t.get(keys[3].partition()), Some(&3));
    }

    #[test]
    fn restore_brings_value_back() {
        let (mut t, keys) = populated();
        t.delete(keys[6].partition());
        assert!(t.restore(keys[6].partition()));
        assert_eq!(t.get(keys[6].partition()), Some(&6));
    }

    #[test]
    fn delete_interior_hides_but_keeps_children() {
        let (mut t, keys) = populated();
        assert!(t.delete(keys[1].partition()));
        assert_eq!(t.get(keys[1].partition()), None);
        assert_eq!(t.get(keys[2].partition()), Some(&2));
        assert_eq!(t.get(keys[7].partition()), Some(&7));
    }

    #[test]
    fn insert_over_deleted_returns_none() {
        let (mut t, keys) = populated();
        t.delete(keys[6].partition());
        let old = t.insert(keys[6].partition(), 99);
        assert_eq!(old, None);
        assert_eq!(t.get(keys[6].partition()), Some(&99));
    }

    #[test]
    fn reload_drops_old_trie_and_builds_new() {
        let (t, _) = populated();
        // Explicitly drop — arena freed, all memory reclaimed.
        drop(t);

        // Build a fresh trie from scratch — no leaks from the old one.
        let mut t2 = CompressedTrie::new();
        let k = t2.parse_key("http.status.200");
        t2.insert(k.partition(), 42usize);
        assert_eq!(t2.get(k.partition()), Some(&42));
    }

    #[test]
    fn sub_partition_lookup() {
        let (t, _) = populated();
        let full = t.parse_key("http.status.404");
        // [1..3] = ["status", "404"] — won't match but exercises slice API
        let sub = full.slice(1, 3);
        assert_eq!(t.get(sub), None);
    }

    // ── structural query API tests ────────────────────────────────────────────

    #[test]
    fn node_at_returns_branch_nodes() {
        // `get` returns None for branch nodes; `node_at` must not.
        let (t, _) = populated();
        assert!(t.get_str("http").is_none(), "get returns None for branch");
        assert!(t.node_at(&["http"]).is_some(), "node_at finds the branch");
        assert!(t.node_at(&["http", "status"]).is_some());
    }

    #[test]
    fn node_at_returns_none_for_missing_path() {
        let (t, _) = populated();
        assert!(t.node_at(&["http", "missing"]).is_none());
        assert!(t.node_at(&["completely", "absent"]).is_none());
        // Partial edge match — "htt" is not a segment boundary.
        assert!(t.node_at(&["htt"]).is_none());
    }

    #[test]
    fn is_branch_at_identifies_multi_child_nodes() {
        let (t, _) = populated();
        // "http" has children: something, status, y  → branch
        assert!(t.is_branch_at(&["http"]));
        // "http.status" has many children → branch
        assert!(t.is_branch_at(&["http", "status"]));
        // "http.status.detail.something.msg" has 2 children → branch
        assert!(t.is_branch_at(&["http", "status", "detail", "something", "msg"]));
    }

    #[test]
    fn is_branch_at_false_for_leaf_and_single_child() {
        let (t, _) = populated();
        // Leaf node — no children.
        assert!(!t.is_branch_at(&["http", "something"]));
        // "http.status.detail" has exactly one child ("something") and is
        // compressed away — no node exists at that path, so is_branch_at is false.
        assert!(!t.is_branch_at(&["http", "status", "detail"]));
        // "http.status.detail.something" has 2 children (msg, notmsg) → IS a branch.
        assert!(t.is_branch_at(&["http", "status", "detail", "something"]));
    }

    #[test]
    fn has_children_at_detects_interior_nodes() {
        // Insert a key and a child of that key.
        let mut t = CompressedTrie::new();
        t.insert_str("a.b", 0usize);
        t.insert_str("a.b.c", 1usize);
        // "a.b" is an Interior — it has a value AND a child.
        assert!(t.has_children_at(&["a", "b"]));
        // "a.b.c" is a Leaf — no children.
        assert!(!t.has_children_at(&["a", "b", "c"]));
    }

    #[test]
    fn children_of_returns_sorted_edges() {
        let (t, _) = populated();
        // Children of "http" should be something, status, y — alphabetical order.
        let kids = t.children_of(&["http"]);
        let labels: Vec<&str> = kids.iter()
            .map(|(edge, _)| edge.as_slice()[0].as_str())
            .collect();
        assert_eq!(labels, vec!["something", "status", "y"]);
    }

    #[test]
    fn children_of_empty_for_leaf() {
        let (t, _) = populated();
        assert!(t.children_of(&["http", "something"]).is_empty());
    }

    #[test]
    fn children_of_empty_for_missing_path() {
        let (t, _) = populated();
        assert!(t.children_of(&["nonexistent"]).is_empty());
    }
}
