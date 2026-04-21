# Refactoring: Hierarchical Render Model + Structured Cursor

> **Archived design proposal.** This document describes the intent and plan for
> the refactoring that was carried out on branch `refactor/hierarchical-model`.
> The implementation diverged from some specifics: `DomainModel` was used instead
> of `RenderModel`; the proposed `Cursor` struct was not introduced — instead the
> cursor is three independent fields on `AppState` (`cursor_row`, `cursor_segment`,
> `cursor_locale`). Treat this as historical context, not a description of current code.

## Motivation

The current architecture stores the render model as a flat `Vec<DisplayRow>` in `AppState`
and navigates it via `cursor_row: usize + CursorSection`. This causes several problems:

- `update` must consult `display_rows` to compute navigation, leaking render structure
  into the domain layer (violates TEA).
- Navigation helpers (`find_depth_neighbor`, `find_anchor_row`, etc.) re-parse
  `full_key` strings with `split(':')` and `split('.')` on every keypress because
  the segment structure was thrown away when keys were stored as flat strings.
- `tui.rs` also re-parses `full_key` on every frame for segment highlighting.
- `preferred_locale`, `key_segment_cursor` (the `segment` offset), and
  `cursor_row` are separate fields that are always updated together but can drift.
- Filter restore-by-identity is a special case inside `apply_filter`; it should be
  the default because the cursor IS its own identity.

## Target Data Structures

### RenderModel

```rust
struct RenderModel {
    bundles: Vec<BundleModel>,
}
```

### BundleModel

```rust
struct BundleModel {
    name:    String,                       // "" for bare/legacy keys
    locales: Vec<String>,                  // locale files present in this bundle
    entries: Vec<Entry>,                   // ALL keys, sorted alphabetically
    groups:  HashMap<String, Group>,       // prefix path → Group (see docs/group-merge.md)
}
```

Entries are sorted by their full dot-joined key path. This ordering is the
load-bearing property that makes rendering and navigation simple (see below).

`groups` is computed by the group-merge scan pass over `entries`. It records
which prefix paths form chain-collapsed visual nodes, their ranges
(`first_entry..=last_entry`), and whether they are branch nodes. The renderer
queries it freely; entries themselves hold no references to groups.

### Entry

```rust
struct Entry {
    segments:  Vec<String>,    // real key split on '.', e.g. ["app","title"]
    cells:     Vec<LocaleCell>, // one per locale in BundleModel.locales
    is_dirty:  bool,
    is_pinned: bool,
    flags:     EntryFlags,     // dangling, temp-pinned, etc.
}

struct LocaleCell {
    value:    Option<String>,  // None = missing
    is_dirty: bool,
}
```

Entries carry no rendering hints. There are no header rows in the model — groups
are structural facts stored separately in `BundleModel.groups` (see below).

### Cursor

```rust
struct Cursor {
    bundle:   Option<String>,  // None = bare/legacy key space
    segments: Vec<String>,     // [] = bundle header level
                               // ["app"] = a group/header position (may have no entry)
                               // ["app","title"] = specific key entry
    locale:   Option<String>,  // None = key column; Some("de") = locale column
}
```

The cursor IS its own identity. No separate `preferred_locale` field is needed —
`cursor.locale` persists and is the preferred locale. Clamping finds the nearest
available locale but leaves `cursor.locale` unchanged so it restores automatically
when a matching bundle is reached.

The cursor can point to positions that have no corresponding entry — e.g.
`segments = ["http","status","detail","something"]` references a group header
even when no key `http.status.detail.something` exists in any file. This is
the clean split: cursor on a group = navigating structure; cursor on an entry
= operating on a value.

`cursor_row: usize` is reduced to a pure rendering detail (for scroll offset and
`draw_table`). It is never stored in `AppState`; the renderer counts visual lines
to find the scroll position of the cursor.

## Navigation Simplifications

With a sorted `Vec<Entry>` per bundle, the navigation helpers that currently work
against the flat `display_rows` string list collapse into simple list operations:

| Current helper | New implementation |
|---|---|
| `find_depth_neighbor(forward)` | scan forward/backward in `bundle.entries` until `entry.segments.len() == cursor.segments.len()`; reduce `cursor.segments` by 1 and retry if exhausted |
| `find_anchor_row` | find entry where `entry.segments == cursor.segments[..n-1]` |
| `find_first_child` | first entry where segments starts with `cursor.segments` and `len == cursor.segments.len() + 1` |
| `key_seg_anchor` | `cursor.segments[..n-k].join(".")` — trivial |
| `key_seg_max` | `cursor.segments.len() - 1` |

No string parsing (`find(':')`, `split('.')`) needed in any of these.

The `segment` offset `k` in `CursorSection::Key { segment }` folds into
`cursor.segments.len()`: walking toward root = `cursor.segments.pop()`,
toward leaf = `cursor.segments.push(next_seg)`.

## Rendering

### No header rows in the model

The render model contains only actual key entries — one per translatable key.
There are no `Header` / `Key` row variants. Groups (the `BundleModel.groups`
map) are structural facts about the data. What to do with them visually is
entirely the renderer's decision.

### How the renderer uses groups

For each entry `i`, the renderer queries:

```
headers_for(i) = [g for g in bundle.groups.values()
                    if g.first_entry == i and g.is_branch]
                 sorted by depth
```

It then decides freely how to present those groups — header lines with
indentation, decorations, collapsed labels, group metadata, whatever. No model
change is required to change the visual representation.

For example, the entry `[http, status, detail, something, msg, firstmessage]`
has two groups starting at it (`detail.something` and `msg`). One possible
rendering:

```
    .detail.something:          ← group header line
      .msg:                     ← group header line
        .firstmessage  [de] …   ← entry key line
```

### Chain collapsing — why a single lookahead is not sufficient

A naive "compare adjacent entries" approach can determine depth and display text
but cannot detect single-child chains with only one entry of lookahead. The
chain `detail → something` spans entries 8–10; when the renderer reaches
entry 8, it cannot tell from entry 9 alone that `detail` will never have any
child other than `something`. The group-merge scan (see `docs/group-merge.md`)
resolves this before rendering by tracking distinct children per open group and
extending group labels in place when a single-child chain closes.

### Visual depth

Visual depth = real segment depth minus the number of depths absorbed by
merged chains above the current node on the path from root. Each `Group`
carries this offset; the renderer reads it to compute indentation.

## Refactoring Order

The data structures are the load-bearing decision. Getting them right first makes
every subsequent step confirm or correct the design.

### Step 1 — Define data structures and build the render model

- Define `RenderModel`, `BundleModel`, `Entry`, `LocaleCell`, `Cursor`.
- Write `build_render_model(workspace, filter) -> RenderModel` replacing
  `build_display_rows`.
- Store `RenderModel` in `AppState` (temporarily alongside `display_rows` if needed
  for a safe incremental migration).

This step should not change any visible behavior. It only restructures data.
Confirm correctness by checking that the rendered rows match the existing output.

### Step 2 — Adapt the renderer

- Rewrite `draw_table` in `tui.rs` to iterate `render_model.bundles → entries`
  instead of `display_rows`.
- Derive `cursor_row` (the flat scroll index) inside `draw_table` by counting
  entries before the cursor — never store it.
- `display_rows` can be removed from `AppState` at this point.

### Step 3 — Rewrite navigation against the new model

- Replace `cursor_row + CursorSection + key_segment_cursor + preferred_locale`
  with `Cursor { bundle, segments, locale }`.
- Rewrite message handlers in `update.rs` that currently manipulate `cursor_row`
  and `cursor_section`.
- The navigation helpers listed above replace `find_depth_neighbor` etc. — they
  are likely short enough to inline into the message handlers directly.
- `apply_filter` loses its cursor restore-by-identity special case; the cursor
  just remains valid.

### Step 4 — Simplify operations

With the structured cursor, operations (`yank`, `edit`, `delete`, `insert`) no
longer need to:
- Look up `display_rows[cursor_row]` for the full key
- Call `split_key` / `locale_idx` chains to get bundle + real key + locale

They read directly from `cursor.bundle`, `cursor.segments.join(".")`, `cursor.locale`.

## Key Design Decisions to Preserve

- **Operations (`ops/`) still work with `full_key: &str`** — the file I/O layer
  uses `bundle:real_key` strings. The cursor provides these on demand;
  `split_key` stays in `ops/` and `filter.rs` where raw strings arrive as parameters.
- **Locale values in the render model** — `LocaleCell` embeds values so the
  renderer is a pure function of the render model, no `workspace.get_value()`
  calls at render time.
- **Rebuild on filter AND on edit** — because locale values are embedded, the
  render model must be rebuilt on every mutation, not only on filter changes.
  This is expected to be fast enough given the entry counts involved.
