# Dirty tracking

## Concept

A key is **dirty** when anything about it has changed in the current session and
those changes have not yet been flushed to disk.  "Anything" means:

- a translation value was added, edited, or deleted for any locale
- the key was created (`n`) and not yet saved
- the key was renamed (the new name is dirty; the old name is gone)

Dirty is tracked at **key level**: one `HashSet<String>` keyed on the
bundle-qualified key name.  The filter DSL already provides locale specificity
through its locale-selector section, so no (key, locale) pair structure is
needed.

## AppState field

```rust
pub dirty_keys: HashSet<String>,
```

Populated automatically by every mutation path (ops/insert, ops/delete,
ops/rename).  Cleared per-key when that key's pending writes are flushed by
`Message::SaveFile`.

## Filter DSL integration

`#` is the dirty sigil.  It can appear in either the key section or the locale
section:

| Filter expression | Meaning |
|---|---|
| `#` | all dirty keys |
| `/confirm#` | keys matching "confirm" AND dirty |
| `/confirm #` | keys matching "confirm" OR dirty |
| `:#` | dirty keys — locale columns unrestricted (same as bare `#`) |
| `:[de]#` | dirty keys, narrowing the visible column set to `de` |
| `:[de?]#` | dirty keys that are also missing a `de` translation |

The locale-section `#` is consistent with how `?` and `!` already work as
per-locale modifiers: they all compose with whatever locale selector precedes
them.

## Relationship to pinning

Dirty and pinned are **orthogonal**:

| | Dirty | Pinned |
|---|---|---|
| Set by | automatically on any mutation | user (`m` key) |
| Cleared by | `Ctrl+S` (save) | user (`M` or `m` to toggle) |
| Purpose | "what changed this session?" | "keep this visible regardless of filter" |
| Filter sigil | `#` | (separate; TBD — e.g. `@` or via a `#pinned` mode) |

Both contribute to row visibility independently:
```
visible = matches_filter(key) OR is_temp_pinned(key) OR is_pinned(key)
```
Dirty does **not** bypass the filter on its own — you use the `#` sigil
explicitly when you want dirty entries surfaced.

## ChildrenAll bulk op receipt

When a `[+children all]` rename completes, the temp-pinned hidden children that
were actually renamed should be added to `dirty_keys` (not `pinned_keys`).
They changed; dirty is the right signal.  The user's filter can then find them
with `#` or `:[de]#` to review the changes.

The current implementation (placeholder) promotes them to `pinned_keys`.  Once
dirty tracking is wired up, the promotion target switches to `dirty_keys` and
the `#` indicator on those rows becomes "dirty" rather than "pinned".

## Implementation order

1. Add `dirty_keys: HashSet<String>` to `AppState`; initialise empty.
2. Populate in every mutation path:
   - `ops::insert::commit_cell_edit` / `commit_cell_insert` → insert key
   - `ops::delete::delete_key_inner` (value deleted from one locale) → insert key
   - `ops::rename::rename_key_in_workspace` → insert new key name
   - `CommitKeyName` (new key created) → insert key
3. Clear in `Message::SaveFile` for keys whose pending writes were successfully
   flushed.
4. Add `FilterExpr::DirtyKey` variant (similar to `DanglingKey`) in `filter.rs`.
5. Parse `#` sigil in key and locale sections.
6. Render the `#` indicator on dirty Key rows (replaces the current
   `pinned_keys`-driven `#`).
7. Switch the `ChildrenAll` receipt from `pinned_keys` to `dirty_keys`.
