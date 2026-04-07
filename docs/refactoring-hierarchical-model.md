# Refactoring: Hierarchical Render Model + Structured Cursor

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
    name:    String,           // "" for bare/legacy keys
    locales: Vec<String>,      // locale files present in this bundle
    entries: Vec<Entry>,       // ALL keys in this bundle, sorted alphabetically
}
```

Entries are sorted by their full dot-joined key path. This ordering is the load-bearing
property that makes rendering and navigation simple (see below).

### Entry

```rust
struct Entry {
    segments:      Vec<String>,             // real key split on '.', e.g. ["app","title"]
    cells:         Vec<LocaleCell>,         // one per locale in BundleModel.locales
    intro_headers: Vec<MergedSegmentRef>,   // merged_segments whose first_entry == this entry,
                                            // ordered shallowest to deepest.
                                            // The renderer emits one header line per element,
                                            // then the key line. See docs/group-merge.md.
    is_dirty:  bool,
    is_pinned: bool,
    flags:     EntryFlags,                  // dangling, temp-pinned, etc.
}

struct LocaleCell {
    value:    Option<String>,  // None = missing
    is_dirty: bool,
}
```

`intro_headers` is the only rendering-related field on `Entry`. The entry itself
has no concept of how many visual lines it will produce — that is entirely the
renderer's concern. The render model carries no separate header rows.

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

### No separate header rows

The render model contains only actual key entries — one per translatable key.
There are no `Header` / `Key` row variants. Visual group headers are a rendering
detail, not a model concern.

### How the renderer produces multiple lines per entry

Each entry carries `intro_headers: Vec<MergedSegmentRef>` (computed by the
group-merge scan pass, see `docs/group-merge.md`). When the renderer reaches
an entry it emits:

1. One header line per element of `intro_headers` (the merged_segment's label
   and visual depth).
2. One key line for the entry itself (segments, locale cells).

The entry does not know how many lines it will produce. The renderer iterates
entries 1:1 and expands them. For example, the entry
`[http, status, detail, something, msg, firstmessage]` has
`intro_headers = [MS_detail·something, US_msg]` and produces three lines:

```
    .detail.something:          ← intro_header line 1
      .msg:                     ← intro_header line 2
        .firstmessage  [de] …   ← key line
```

### Chain collapsing — why a single lookahead is not sufficient

A naive "compare adjacent entries" approach determines depth and display text
correctly but cannot detect single-child chains with only one entry of lookahead.
The chain `detail → something` spans entries 8–10; when the renderer reaches
entry 8, it cannot tell from entry 9 alone that `detail` will never have any
child other than `something`. The group-merge scan, which runs before rendering,
resolves this by tracking distinct children per open group and firing merges when
a group closes.

### Visual depth

Visual depth = real segment depth minus the number of depths absorbed by merged
chains above the current node on the path from root. Each `MergedSegment`
carries an `absorbed` count (number of unique_segments in its chain minus 1).
This is pre-computed during the scan pass and stored on the `MergedSegment`,
so the renderer reads it directly.

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
