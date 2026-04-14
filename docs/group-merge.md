# Group-Merge: Segment Group Detection

> **Alternative approach — not implemented.** The current code uses a
> `CompressedTrie` (`radix_tree_arena.rs`) to detect chain-collapse boundaries
> via `key_partitions()`. Group-merge is a simpler single-forward-scan algorithm
> that achieves the same result given one precondition: entries must be sorted
> alphabetically by key. We currently satisfy that precondition (`merged_keys` is
> always sorted), but may not always. The algorithm is documented here for
> reference — it is clean, well-understood, and worth keeping in mind if the trie
> ever becomes a bottleneck or the sorting assumption is re-evaluated.

## Context

This idea came up while designing the hierarchical render model
(see `docs/refactoring-hierarchical-model.md`). It is a single-forward-scan
algorithm for detecting which key segments always appear and disappear together
across a sorted entry list, enabling chain collapsing without a trie.

The scan produces a flat map of prefix paths → groups and a vec of group objects.
This is pure structural data about the keys. The renderer queries it freely to
decide how to present groups visually — the model makes no rendering decisions.

## The Problem It Solves

Given a sorted list of entries with pre-split segments, the renderer needs to
know which adjacent depth levels can be collapsed into a single visual node.
For example:

```
[http, status, detail, something, msg, firstmessage]
[http, status, detail, something, msg, secondmessage]
[http, status, detail, something, notmsg]
[http, status, x]
```

`detail` (depth 2) and `something` (depth 3) always appear and disappear
together — they form a single-child chain and should be collapsed into one
visual node `detail.something:`. `msg` (depth 4) has two children so it
becomes its own header node.

### Why a single one-entry lookahead is not enough

When the renderer reaches the first entry `[…, detail, something, msg, …]`,
it cannot determine from the next entry alone whether `detail` will chain into
`something`. The `detail` group spans entries 8–10; only when it closes (at
entry 11) do we know it had exactly one child. The scan must track open groups
across entries — not just peek one ahead.

## Data Structures

```
Group {
    label:       String        // "detail", extended to "detail.something" on merge
    depth:       int           // depth of the shallowest segment in this group
    first_entry: int           // index of first entry in this group's range
    last_entry:  int           // index of last entry (set when group closes)
    children:    List[Group]   // direct child groups (populated as children close)
    is_branch:   bool          // true when children.len() > 1
}

map: HashMap<String, Group>    // "http.status.detail" → Group
                               // both the original and merged prefixes point to
                               // the same Group object after a merge
```

Each `Entry` holds no group references — the renderer looks up groups by
querying `map` or by checking `group.first_entry`.

## Three-Level Indirection (naming)

It helps to name the three levels:

```
key_segment      one occurrence of a segment value in one entry at one depth
                 (implicit — entry.segments[d])

unique_segment   a contiguous run sharing the same segment value at the same
                 depth. Represented as a Group before any merging.
                 "All the times `detail` appeared at depth 2 in entries 8–10."

merged_segment   a Group after its label has been extended by chain collapsing.
                 "detail" becomes "detail.something" in place — same object,
                 updated label. Both map["…detail"] and map["…detail.something"]
                 point to it.
```

Each unique_segment starts as its own merged_segment ("merging with itself").
Chain detection promotes some of them by extending their label in place.

## The Scan Algorithm

### State

- A stack of open Groups, one slot per active depth level.
- Each open Group tracks its `children` list (populated as children close).

### For each entry i

```
shared = common_prefix_length(entry.segments, prev_entry.segments)
         # 0 for the first entry

# Close depths >= shared, DEEPEST FIRST
for d from (stack.len - 1) down to shared:
    g = stack[d]
    g.last_entry = i - 1

    if d > 0:
        parent = stack[d - 1]
        parent.children.append(g)

        # Merge check: runs when PARENT closes (not here).
        # By closing deepest-first, all of a parent's children are recorded
        # before the parent is evaluated.

    # Single-child chain check (parent closed, check its one child)
    if g.children.len() == 1:
        child = g.children[0]
        if child.first_entry == g.first_entry and child.last_entry == g.last_entry:
            # Extend in place — no new object
            g.label += "." + child.label
            g.depth = min(g.depth, child.depth)
            # Redirect map so child's prefix also resolves to g
            map[child_prefix] = g
            # child object is now unreferenced (or can be tombstoned)
        else:
            g.is_branch = (g.children.len() > 1)
    else:
        g.is_branch = (g.children.len() > 1)

    stack.pop()

# Open new depths
for d from shared to entry.segments.len() - 1:
    prefix = entry.segments[..=d].join(".")
    g = Group(label=entry.segments[d], depth=d, first_entry=i)
    map[prefix] = g
    stack.push(g)
```

After the full scan, flush the stack with a sentinel (empty entry).

## Worked Example

Input (bundle `stripping`, entries sorted):
```
0:  [http, something]
1:  [http, status]
2:  [http, status, 200]
3–7:[http, status, 400/401/403/404/500]
8:  [http, status, detail, something, msg, firstmessage]
9:  [http, status, detail, something, msg, secondmessage]
10: [http, status, detail, something, notmsg]
11: [http, status, x]
12: [http, y]
```

Key moments:

**Entry 8** (divergence at d=2, `detail` ≠ `500`):
- Close G_500 (leaf).
- Open G_detail(d=2), G_something(d=3), G_msg(d=4), G_firstmsg(d=5).

**Entry 9** (divergence at d=5):
- Close G_firstmsg (leaf). G_msg.children = {G_firstmsg}.

**Entry 10** (divergence at d=4):
- Close G_secondmsg (leaf).
- Close G_msg: children = {G_firstmsg, G_secondmsg} → **2 children → branch, no merge**.

**Entry 11** (divergence at d=2), deepest first:
- Close G_notmsg (leaf).
- Close G_something: children = {G_msg, G_notmsg} → **2 children → branch, no merge**.
  Add G_something to G_detail.children.
- Close G_detail: children = {G_something}, ranges match (8–10 == 8–10)
  → **MERGE**: extend G_detail in place.
  `G_detail.label = "detail.something"`.
  `map["http.status.detail.something"] = G_detail`.
  G_something is abandoned.

**Entry 12** (divergence at d=1):
- Close G_x (leaf). G_status.children gets G_x.
- Close G_status: 8 children → **branch, no merge**.

**End**: Close G_y (leaf), G_http: 3 children → branch, no merge.

### Resulting groups (relevant subset)

```
map["http"]                          → G_http         {label:"http",             branch, range:0–12}
map["http.something"]                → G_something_A  {label:"something",        leaf,   range:0–0}
map["http.status"]                   → G_status        {label:"status",           branch, range:1–11}
map["http.status.200"]               → G_200           {label:"200",              leaf,   range:2–2}
  ... (401–500 same)
map["http.status.detail"]            → G_detail        {label:"detail.something", branch, range:8–10}
map["http.status.detail.something"]  → G_detail        {same object}
map["http.status.detail.something.msg"] → G_msg        {label:"msg",              branch, range:8–9}
map["http.status.x"]                 → G_x             {label:"x",                leaf,   range:11–11}
map["http.y"]                        → G_y             {label:"y",                leaf,   range:12–12}
```

## What the Renderer Does With This

The renderer iterates entries and, for each entry `i`, queries the groups to
find which ones start here:

```
headers_for(i) = [g for g in groups if g.first_entry == i and g.is_branch]
                 sorted by depth
```

For entry 8 that yields `[G_detail ("detail.something", d=2), G_msg ("msg", d=4)]`.
The renderer decides freely how to present those — as indented header lines, as
decorations, as a collapsed label, anything. The model carries no rendering intent.

### Visual depth

Visual depth of a group or entry = real depth minus depths absorbed by merged
chains above it on the path from root. G_detail absorbed 1 depth (detail+something
→ one visual level), so `msg` at real depth 4 renders at visual depth 3, and
`firstmessage` at real depth 5 renders at visual depth 4.

## Potential Uses Beyond Rendering

**Structural highlighting** — `group.first_entry..=last_entry` is a range.
When the cursor is inside a group, highlight all entries in that range. O(1) check.

**Scope operations** — `[+children]` rename/delete scope maps directly to the
group range. No scan needed at operation time.

**Cursor group identity** — `cursor.segments = ["http","status","detail","something"]`
references G_detail even though no key `http.status.detail.something` exists as
an entry. The cursor can navigate to group headers without needing a backing entry.

**Stable group IDs** — if groups are indexed deterministically by their prefix
path, they survive filter rebuilds. The cursor holds its position across rebuilds
with O(1) map lookup.
