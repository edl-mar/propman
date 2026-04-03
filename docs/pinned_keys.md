# Pinned keys

## Problem

Two related issues arise when a filter is active:

1. **Post-commit disappearance** — after editing/renaming a key, it may no longer match
   the filter and vanishes from view. The user loses track of what they just changed.

2. **Invisible bulk edits** — a prefix rename or prefix delete with `+children` scope
   silently modifies keys that are hidden by the filter. The user had no idea those
   entries were in scope.

## Design

### Pin state (AppState)

```rust
pub temp_pins: Vec<String>,        // current op only; discarded on mode exit
pub pinned_keys: HashSet<String>,  // persistent; user-controlled
```

Visibility rule: `visible = matches_filter(key) OR is_pinned(key)`
where `is_pinned` checks both `temp_pins` and `pinned_keys`.

### Pinned mode (filter bar)

`#` in the filter bar → `visible = is_pinned(key)` only.
Shows all pinned entries at once, regardless of other filter state.

### Manual pinning (Normal mode)

- `m` on a Key row: pin/unpin that exact key
- `m` on a Header row (or with selection scope set to `+children`): pin/unpin the whole prefix subtree
- `M`: drop all entries from `pinned_keys` at once
- Pinned entries are marked with a visible indicator in the key column

### Bulk op integration (KeyRenaming / Deleting)

When entering `KeyRenaming` or `Deleting` with `+children` scope and a filter is
active, hidden children that would be affected are **temporarily pinned** so they
surface in the main table. This lets the user see the real scope of the operation.

Tab cycles through three scope states:

| State | Hidden affected entries | Will be modified |
|---|---|---|
| `exact` | not shown (only one key in scope) | no |
| `+children [filtered]` | shown, dimmed gray | no |
| `+children [all]` | shown, dim green | yes |

Coloring comes from the **op context** (is the entry in scope?), not from the pin
state. The pin is only the mechanism that makes them visible.

**On commit:**
- `[filtered]`: `temp_pins` discarded → hidden entries go back to being hidden
- `[all]`: entries that were actually changed → promoted from `temp_pins` to
  `pinned_keys` (visible receipt of what was touched); rest discarded

**On cancel:** all `temp_pins` discarded.

### Rendering

| Row type | Style |
|---|---|
| Normal filtered row | default |
| Temp-pinned, out of scope | dimmed / gray |
| Temp-pinned, in scope | dim green |
| Permanently pinned | pin indicator in key column, otherwise normal |

## Implementation order

1. `pinned_keys: HashSet<String>` + `temp_pins: Vec<String>` on `AppState`
2. Update `apply_filter` / visibility rule to include pinned keys
3. Manual pin/unpin (`p` / `P`) in Normal mode
4. `#` pinned mode in filter bar
5. Bulk op temp-pinning: populate `temp_pins` on entering `+children` scope,
   promote/discard on commit/cancel
6. Rendering: dimmed gray / dim green for temp-pinned rows; pin indicator for
   permanent pins
