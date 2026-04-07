# Group-Merge: Segment Group Detection

## Context

This idea came up while designing the hierarchical render model
(see `docs/refactoring-hierarchical-model.md`). It is a more general algorithm
for detecting which key segments always appear and disappear together across a
sorted entry list. It is NOT required for the core refactoring but may be useful
for rendering, structural highlighting, and cursor group identity.

## The Problem It Solves

Given a sorted list of entries with pre-split segments, the rendering needs to
know which adjacent depth levels can be collapsed into a single visual node.
For example:

```
[http, status, detail, something, msg, firstmessage]
[http, status, detail, something, msg, secondmessage]
[http, status, detail, something, notmsg]
[http, status, x]
```

`detail` (depth 2) and `something` (depth 3) always appear and disappear together
across entries 0-2 — they form a single-child chain and should be shown as one
visual node `detail.something:`. `msg` (depth 4) spans only entries 0-1 and has
two children, so it becomes its own header. This is the chain-collapsing logic
that `render_model.rs` currently implements via a trie walk.

## The Algorithm

### Concept

Assign every segment occurrence a **group**. A group is a contiguous run of
entries that share the same segment value at the same depth. When a group at
depth D and the group at depth D+1 have exactly the same entry range (same start
index, same end index), they can be merged — they form a single-child chain.

### Data Structures

```
Group {
    segment:     String,        // the segment value, e.g. "detail"
    depth:       usize,         // depth in the key (0-based)
    first_entry: usize,         // index of first entry containing this segment
    last_entry:  usize,         // index of last entry containing this segment (inclusive)
    parent:      Option<GroupId>,
    children:    Vec<GroupId>,  // direct child groups
    merged_into: Option<GroupId>, // set when this group is collapsed into its parent
}
```

Each `Entry` carries a `Vec<GroupId>` — one group ID per depth level.

### Forward Pass

Walk the sorted entry list. At each depth level of each entry:
- If the segment at this depth is the same as the previous entry's segment at this
  depth, reuse the current open group for this depth: extend its `last_entry`.
- Otherwise, close the current group for this depth (and all deeper depths, since
  the prefix changed) and open a new group.

When a group closes, check the merge condition:

### Merge Condition

A group G at depth D can be merged into its parent group P (depth D-1) when:
- G and P have the same `first_entry` and `last_entry` (they span the same entries)
- P has exactly one child group (G is P's only child across its entire range)

If the condition holds, set `G.merged_into = Some(P.id)` and extend P's display
label to include G's segment: `P.display = format!("{}.{}", P.display, G.segment)`.

### Lookahead

The merge check requires knowing G's `last_entry`, which is only known when G
closes. G closes when the next entry diverges at depth D. So the check runs with
a one-entry lookahead: when processing entry `i`, close and check all groups whose
depth is >= the divergence depth between entry `i-1` and entry `i`.

### Avoiding Retroactive Updates

When G is merged into P, you don't need to update the `GroupId` stored in every
previous entry for depth D. Instead, add a second level of indirection: store a
`canonical_id(group_id)` function that follows `merged_into` links to the root.
The group a segment "really belongs to" is the root of its merge chain.

## Derived Information

Once the pass completes, each group (after following merge links) gives you:

- **Is it a header?** A group is a header if it has more than one child group, OR
  if it is merged (multi-segment chain) and its merged root has more than one child.
- **Indentation depth** = root group's depth (after merges).
- **Display label** = root group's accumulated display string (e.g. `detail.something`).
- **Entry range** = `first_entry..=last_entry` — all entries that fall under this group.
- **Group identity for the cursor** — the cursor can reference a group directly,
  not just a specific entry. This gives a stable handle for navigating to "the
  `detail.something` group" even when it contains no standalone key entry.

## Potential Uses

### Structural Highlighting

When the cursor is inside a group, all entries in that group's range can be
highlighted as "in scope". The group boundary (first/last entry index) makes
this a simple range check.

### Cursor Group Identity

With the structured cursor (`Cursor { bundle, segments, locale }`),
`cursor.segments` already encodes the group implicitly. But the group object
provides the range directly, avoiding a scan. Useful for:
- Jump to first/last entry in the current group
- Highlight the whole group when a scope operation is active

### Header Rows Without a Trie

The groups that survive as non-merged headers (multiple children) are exactly the
header rows that need to be rendered. Generating the visual row list from the group
forest is a simple tree walk — no trie construction needed.

### Future: Inter-Bundle Navigation

If group IDs are stable across filter changes (assigned deterministically from the
segment path), the cursor can hold a group reference that survives rebuilds —
similar to how `cursor.segments` survives filter changes today, but with O(1)
group lookup instead of a linear search.

## Relationship to the Simple Lookahead Approach

For the rendering pass alone, the one-entry lookahead described in
`refactoring-hierarchical-model.md` (compare adjacent entries, check if next
entry shares prefix) is sufficient and simpler to implement. The group-merge
algorithm is worth building if the derived information (group ranges, stable IDs,
multi-use across rendering + navigation + highlighting) justifies the added
complexity. It is not a prerequisite for the core refactoring.
