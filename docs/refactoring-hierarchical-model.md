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
    segments: Vec<String>,          // real key split on '.', e.g. ["app","title"]
    cells:    Vec<LocaleCell>,      // one per locale in BundleModel.locales
    // derived / pre-computed for rendering:
    is_dirty:  bool,
    is_pinned: bool,
    flags:     EntryFlags,          // dangling, temp-pinned, etc.
}

struct LocaleCell {
    value:    Option<String>,  // None = missing
    is_dirty: bool,
}
```

### Cursor

```rust
struct Cursor {
    bundle:   Option<String>,  // None = bare/legacy key space
    segments: Vec<String>,     // [] = bundle header level
                               // ["app"] = within-bundle group/header level
                               // ["app","title"] = specific key
    locale:   Option<String>,  // None = key column; Some("de") = locale column
}
```

The cursor IS its own identity. No separate `preferred_locale` field is needed —
`cursor.locale` persists and is the preferred locale. Clamping finds the nearest
available locale but leaves `cursor.locale` unchanged so it restores automatically
when a matching bundle is reached.

`cursor_row: usize` is reduced to a pure rendering detail (for scroll offset and
`draw_table`). It is derived from the cursor by finding the matching entry in the
bundle model — never stored in `AppState`.

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

## Rendering Simplifications

With entries sorted and segments pre-split, the trie in `render_model.rs` is not
needed. The display structure is derived in a single linear pass:

For each entry `i`, compare `entry[i].segments` with `entry[i-1].segments`:
- **Shared prefix length** = first index where they differ → indentation depth
- **Visible segments** = `entry[i].segments[shared_prefix_len..]` → display text
- **Is parent** = `entry[i+1].segments.starts_with(entry[i].segments)` → header marker

Chain collapsing (e.g. `detail.something:` as one visual node) is handled by a
one-entry lookahead: at each new depth level being introduced, check whether all
entries in the upcoming range share the same segment at that depth. If so, it is a
single-child chain — merge it with the next level into one display label.

See `docs/group-merge.md` for a more general algorithm that detects these chains
and has additional uses (structural highlighting, cursor group identity).

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
