# Group-Merge: Segment Group Detection

## Context

This idea came up while designing the hierarchical render model
(see `docs/refactoring-hierarchical-model.md`). It is a single-forward-scan
algorithm for detecting which key segments always appear and disappear together
across a sorted entry list, enabling chain collapsing without a trie. The scan
results are stored on each `Entry` as `intro_headers`, which the renderer uses
to produce multiple visual lines per entry without the render model itself
knowing anything about visual structure.

## The Problem It Solves

Given a sorted list of entries with pre-split segments, the renderer needs to
know which adjacent depth levels can be collapsed into a single visual node.
For example, given:

```
[http, status, detail, something, msg, firstmessage]
[http, status, detail, something, msg, secondmessage]
[http, status, detail, something, notmsg]
[http, status, x]
```

`detail` (depth 2) and `something` (depth 3) always appear and disappear
together — they form a single-child chain and should be shown as one visual
node `detail.something:`. `msg` (depth 4) has two children so it becomes its
own header. This is the chain-collapsing logic that `render_model.rs` currently
implements via a trie walk; the group-merge algorithm replaces the trie with a
single forward scan.

### Why a single one-entry lookahead is not enough

When the renderer reaches the first entry `[http, status, detail, something,
msg, firstmessage]`, it cannot determine from the next entry alone whether
`detail` will chain into `something`. The `detail` group spans entries 0–2, and
only when it closes (at entry 3) do we know it had exactly one child. The chain
label must be known before any of the child rows are emitted. This requires
tracking open groups across entries — not just a one-entry peek.

## Three-Level Indirection

The algorithm introduces three named concepts to avoid O(n) retroactive updates:

```
key_segment      one occurrence of a segment value in one entry at one depth
                 (implicit — just entry.segments[d])

unique_segment   a contiguous run of entries that share the same segment value
                 at the same depth. Created when a new segment first appears,
                 closed when the next entry diverges. Represents "all the times
                 `detail` appeared at depth 2 while that group was active".

merged_segment   the visual node shown to the user. Initially each unique_segment
                 is its own merged_segment ("merging with itself"). When a
                 single-child chain is detected, two merged_segments fuse into
                 one with a combined label (e.g. "detail.something").
```

Each entry implicitly references a `unique_segment` per depth level. Each
`unique_segment` holds a pointer to its `merged_segment`. When a merge happens,
only that pointer is updated — O(1). The entries themselves are never touched.
The resolver follows: `entry.segments[d]` → `unique_segment` → `merged_segment`.

## The Scan Algorithm

### State

- A stack of open `unique_segment`s, one slot per active depth level.
- Each `unique_segment` tracks: `first_entry`, `last_entry`, `distinct_children`
  (the set of distinct child unique_segments seen while it was open).

### For each entry i

1. Compare `entry[i].segments` with `entry[i-1].segments` to find the divergence
   depth `d` (first index where they differ; 0 for the first entry).
2. **Close** all open unique_segments at depths `>= d`, processing deepest first.
   For each closing unique_segment U:
   - Record `U.last_entry = i - 1`.
   - Add U to its parent's `distinct_children`.
   - **Merge check** (only after adding to parent):
     - If `U.parent.distinct_children.len() == 1`
       AND `U.first_entry == U.parent.first_entry`
       AND `U.last_entry  == U.parent.last_entry`
       → the parent is a single-child chain into U: merge them.
       Create a new `merged_segment` with label
       `format!("{}.{}", parent.label, U.segment)`. Set both
       `parent.merged_into` and `U.merged_into` to the new merged_segment.
       Two pointer writes; no entry updates.
   - (The merge check for U's parent runs when U's parent closes, using the
     same rule. Closing deepest-first ensures children are fully recorded
     before the parent is evaluated.)
3. **Open** new unique_segments for depths `d .. entry[i].segments.len()`.
   Each new unique_segment starts as its own merged_segment.

After the full scan, close any still-open unique_segments as if a sentinel
entry with empty segments arrived.

### Post-scan: intro_headers on each Entry

After the scan, for each unique_segment U, set `U.first_entry` on the
corresponding entry: the entry at index `U.first_entry` is responsible for
emitting U's canonical merged_segment header (if it is a branch). Collect
these into `entry.intro_headers: Vec<MergedSegmentRef>`, ordered shallowest
to deepest. Merged unique_segments that resolve to the same merged_segment are
deduplicated — only one header is emitted per merged_segment per entry.

## Worked Example

Input:
```
0: [http, something]
1: [http, status]
2: [http, status, 200]
3: [http, status, 400]   (entries 4–7: 401, 403, 404, 500 — same pattern)
8: [http, status, detail, something, msg, firstmessage]
9: [http, status, detail, something, msg, secondmessage]
10:[http, status, detail, something, notmsg]
11:[http, status, x]
12:[http, y]
```

Key moments in the scan:

**Entry 8** — divergence at depth 2 (`detail` ≠ `500`):
- Close US_500 (leaf, no merge).
- Open US_detail(d=2), US_something(d=3), US_msg(d=4), US_firstmessage(d=5).
- children(US_detail) = {US_something}, children(US_something) = {US_msg}, etc.

**Entry 9** — divergence at depth 5 (`secondmessage` ≠ `firstmessage`):
- Close US_firstmessage (leaf, no merge).
- Open US_secondmessage. children(US_msg) = {US_firstmessage, US_secondmessage}.

**Entry 10** — divergence at depth 4 (`notmsg` ≠ `msg`):
- Close US_secondmessage (leaf).
- Close US_msg: children = {firstmessage, secondmessage} → **2 children → branch, no merge**.
- Open US_notmsg. children(US_something) = {US_msg, US_notmsg}.

**Entry 11** — divergence at depth 2 (`x` ≠ `detail`), close deepest first:
- Close US_notmsg (leaf).
- Close US_something: children = {US_msg, US_notmsg} → **2 children → branch, no merge**.
  Add US_something to US_detail.distinct_children.
- Close US_detail: children = {US_something} → **1 child**.
  Range check: US_detail(8–10) == US_something(8–10) ✓
  → **MERGE**: create MS_detail·something(label="detail.something").
  Set US_detail.merged_into = MS_detail·something.
  Set US_something.merged_into = MS_detail·something.
  Two pointer writes.
- Open US_x.

**End of entries** — close remaining: US_x (leaf), US_status (8 children, no
merge), US_y (leaf), US_http (3 children, no merge).

### Resulting intro_headers per entry

| Entry | intro_headers (canonical merged_segments first seen here) |
|---|---|
| 0 `[http, something]` | MS_http (branch), US_something (leaf → no header) |
| 1 `[http, status]` | MS_status (branch) |
| 2 `[http, status, 200]` | — |
| 8 `[http, status, detail, something, msg, firstmessage]` | MS_detail·something (branch), US_msg (branch) |
| 9 `[http, status, detail, something, msg, secondmessage]` | — |
| 10 `[http, status, detail, something, notmsg]` | — |
| 11 `[http, status, x]` | — |
| 12 `[http, y]` | — |

### Visual output produced by the renderer

```
http:                          ← intro_header of entry 0  (MS_http, depth 0)
  .something                   ← key line    of entry 0
  .status:                     ← intro_header of entry 1  (MS_status, depth 1)
    .200                       ← key line    of entry 2
    .400  .401  …  .500        ← key lines   of entries 3–7
    .detail.something:         ← intro_header of entry 8  (MS_detail·something, depth 2)
      .msg:                    ← intro_header of entry 8  (US_msg, visual depth 3)
        .firstmessage          ← key line    of entry 8
        .secondmessage         ← key line    of entry 9
      .notmsg                  ← key line    of entry 10
    .x                         ← key line    of entry 11
  .y                           ← key line    of entry 12
```

Entry 8 produces **three visual lines**: two intro_headers followed by its key
line. The render model has no concept of this — it just has an entry with an
`intro_headers` list of length 2. The renderer iterates entries 1:1 and emits
`intro_headers.len() + 1` lines per entry.

### Visual depth

Visual depth = real depth minus the number of depths absorbed by merged chains
above this node on the path from root. Each merged_segment carries an
`absorbed` count (= number of unique_segments in its merge chain minus 1).

For `msg` at real depth 4: MS_detail·something absorbed 1 depth (2 US → 1
visual level) on the path from root. Visual depth = 4 − 1 = **3** ✓.
For `firstmessage` at real depth 5: same 1 absorbed. Visual depth = 5 − 1 = **4** ✓.

## Relationship to the Render Model and Cursor

### No header rows in the render model

The render model (`BundleModel`) contains only actual key entries. There are no
separate header rows. Headers are a rendering detail derived from `intro_headers`
on each entry. The render model stays clean: one entry per translatable key.

### Cursor positions for groups

With the structured cursor (`Cursor { bundle, segments, locale }`), the cursor
can reference a merged_segment position directly:
- `cursor.segments = ["http","status","detail","something"]` is a valid cursor
  position even though no key `http.status.detail.something` exists. It refers
  to the group header rendered by MS_detail·something.
- `cursor.segments = ["http","status","detail","something","msg","firstmessage"]`
  refers to the actual entry.

This gives a clean split: cursor on a group = navigating structure; cursor on
an entry = editing a value. No ambiguity, no need for a separate `Header` row type.

## Potential Uses Beyond Rendering

### Structural Highlighting

`merged_segment.first_entry..=last_entry` is a range of entry indices. When the
cursor is inside a group, highlight all entries in that range. O(1) range check.

### Scope Operations

`[+children]` scope (for rename, delete) maps directly to the group range. No
scan needed — the group already knows its extent.

### Cursor Group Identity

The group object provides the range directly, avoiding a linear scan when
jumping to the first or last entry in a group.

### Future: Stable Group IDs

If group IDs are assigned deterministically from the segment path, they survive
filter rebuilds. The cursor can hold a group reference with O(1) lookup.
