# Refactoring log

## What was done (April 2026)

`update.rs` had grown to 1234 lines mixing dispatch, business logic, and
state helpers all in one place.  The goal was to separate concerns without
introducing unnecessary abstractions.

### Approach

Rather than a mode-struct abstraction (considered and rejected — see below),
the split was purely by role:

- **Business logic** → `src/ops/` (three files, each independently testable)
- **AppState helpers** → methods on `AppState` in `src/state.rs`
- **Dispatch** → `update.rs` stays as the single `match (mode, msg)` table

### Result

| File | Before | After |
|---|---|---|
| `update.rs` | 1234 lines | 453 lines |
| `state.rs` | 123 lines | 202 lines |
| `ops/delete.rs` | — | 154 lines |
| `ops/insert.rs` | — | 238 lines |
| `ops/rename.rs` | — | 280 lines |

### ops/ module

```
ops/delete.rs   — delete_key_inner, delete_key, delete_key_prefix,
                  delete_locale_entry
ops/insert.rs   — insert_into_file, commit_cell_edit, commit_cell_insert
ops/rename.rs   — commit_exact_rename, commit_prefix_rename,
                  commit_cross_bundle_rename, commit_cross_bundle_prefix_rename,
                  rename_key_in_workspace
```

All functions take `&mut AppState` — no mode or message knowledge.

### AppState methods added (state.rs)

```
apply_filter()       — re-evaluates filter, rebuilds view_rows + visible_locales
current_cell_value() — value at current cursor cell, or None
current_row_bundle() — bundle name for the current row
clamp_scroll()       — keeps scroll_offset in sync with cursor_row
```

---

## What was considered and not done

### Mode-struct abstraction

The original plan proposed wrapping each `Mode` variant in a struct that
owns its sub-state and exposes a `handle(self, msg, &mut state) -> Mode`
method.  This was rejected because:

- Moving `state.mode` out while passing `&mut state` requires a
  `mem::replace` dance on every dispatch call.
- The renderer and keybindings code would need to unwrap inner structs
  to inspect the mode.
- At 453 lines the dispatch is already readable; the abstraction would
  add indirection without adding clarity.

### modes/ split

Splitting `update.rs` into `modes/normal.rs`, `modes/editing.rs`, etc.
(each with a `pub fn handle(state: &mut AppState, msg: Message)`) was
considered as a follow-up step.  Deferred: at the current size each mode
block is ~40-70 lines and has a clear section header.  Worth revisiting
when a specific mode grows significantly.

### Folding Continuation into EditingMode

`Mode::Continuation` is still a top-level variant.  With the modes/ split
it could become an `EditingPhase` enum internal to `modes/editing.rs`,
removing it from the global `Mode` enum.  Deferred along with modes/.

### pinned_key

When a filter is active and an operation touches keys outside the current
view (bulk rename/delete of a prefix), invisible entries are modified
silently and visible entries may disappear mid-operation.  A `pinned_key`
field on `AppState` would override the filter for the duration of an edit,
keeping the affected rows visible.  This is a feature addition, not a
refactor — tracked separately.
