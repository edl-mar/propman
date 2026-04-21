# Pinned keys

## Problem

Two related issues arise when a filter is active:

1. **Post-commit disappearance** — after editing/renaming a key, it may no longer match
   the filter and vanishes from view. The user loses track of what they just changed.
   → Solved by dirty tracking: dirty keys bypass the filter automatically.

2. **Invisible bulk edits** — a prefix rename or prefix delete with `+children all` scope
   silently modifies keys that are hidden by the filter.
   → Solved by temp-pins (surfacing) + dirty tracking (post-op receipt).

Manual pinning is a separate, persistent bookmark mechanism for keys the user
wants to keep visible across sessions or operations, independent of dirty state.

## Design

### Pin state (AppState)

```rust
// D6 (architectural_debt.md): migrate to Vec<KeyId> / HashSet<KeyId> once key.rs is retired
pub temp_pins: Vec<String>,        // current op only; discarded on mode exit
pub pinned_keys: HashSet<String>,  // persistent; user-controlled via m/M
```

Visibility rule:
```
visible = matches_filter(key) OR is_dirty(key) OR is_temp_pinned(key) OR is_pinned(key)
```

Pinning is **not** the same as dirty (see `docs/dirty.md`):
- `dirty_keys` = auto-tracked unsaved changes; bypass filter until saved; `#` filter sigil.
- `pinned_keys` = explicit user bookmarks; bypass filter until manually unpinned; `@` indicator.

### Manual pinning (Normal mode)

- `m` on any row: pin/unpin that exact key (or header prefix)
- Pinned entries show an `@` prefix in the key column
- `M` (clear all pins) is not yet bound

### Temp-pins and ChildrenAll scope

Tab cycles through three scope states in KeyRenaming and Deleting modes:

| State | Hidden affected entries | Will be modified |
|---|---|---|
| `exact` | not shown | no |
| `+children` | not shown; silently unaffected | no |
| `+children all` | temp-pinned on scope enter | yes |

When `+children all` is active, hidden children of the cursor key are added to
`temp_pins` so they surface in the table.  This lets the user see the real scope
of the operation before committing.

**On commit (`+children all`):** `temp_pins` is cleared; the modified keys are
automatically marked dirty and remain visible via the dirty bypass.

**On commit (`+children`):** `temp_pins` cleared (none were set).

**On cancel:** all `temp_pins` discarded; `apply_filter` re-run.

### Rendering

| Row type | Key column style |
|---|---|
| Normal | default |
| Dirty (unsaved changes) | yellow, `#` prefix |
| Temp-pinned, out of scope | dim green |
| Temp-pinned, in scope | dim green (same — always in scope for ChildrenAll) |
| Permanently pinned | `@` prefix |

## Implementation order

1. ✅ `pinned_keys: HashSet<String>` + `temp_pins: Vec<String>` on `AppState`
2. ✅ Update `apply_filter` / visibility rule to include pinned keys and dirty keys
3. ✅ `ChildrenAll` temp-pin surfacing: populate `temp_pins` on entering `+children all` scope
4. ✅ Discard temp_pins on commit/cancel
5. ✅ Manual pin/unpin (`m`) keybinding and message handler
6. ✅ `@` indicator rendering for permanently pinned rows
