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
needed at the state level.  Per-cell dirty state (for rendering `#[de]` tags) is
derived at render time from `pending_writes` + a path→locale map.

## AppState fields

```rust
pub dirty_keys: HashSet<String>,
```

Populated automatically by every mutation path (ops/insert, ops/delete,
ops/rename).  Cleared per-key when that key's pending writes are flushed by
`Message::SaveFile`.

## Visibility

Dirty keys **bypass the filter** — they are always visible regardless of the
active filter expression, just like `temp_pins` and `pinned_keys`.  This means
edited entries never disappear from view while they have unsaved changes.  Once
saved, the entry reverts to normal filter-based visibility.

## Rendering

- **Key column**: dirty rows shown in yellow with a `#` prefix before the key name.
- **Locale tags**: a pending write for a specific `(key, locale)` pair renders the
  locale tag as `#[de]` (yellow) instead of `[de]` (dark gray).

## Filter DSL integration

`#` is the dirty sigil.  It is recognised in the key section and the locale section:

| Filter expression | Meaning |
|---|---|
| `/#` | all dirty keys |
| `/confirm#` | keys matching "confirm" AND dirty |
| `:#` | dirty keys — locale columns unrestricted (same as `/#`) |
| `:[de]#` | dirty keys, narrowing the visible column set to `de` |
| `:[de?]#` | dirty keys that are also missing a `de` translation |

All filter terms are ANDed together.  There is no OR support yet (`FilterExpr::Or`
is reserved but unimplemented).  The `#` sigil is therefore useful for narrowing,
not for "always show dirty regardless of other terms" — that is handled by the
automatic bypass described above.

Note: `#` in the bundle section (before any `/`) is treated as a bundle name
pattern and will match nothing.  Always use `/#` or `:#`.

## Relationship to pinning

Dirty and pinned are **orthogonal**:

| | Dirty | Pinned |
|---|---|---|
| Set by | automatically on any mutation | user (`m` key, not yet implemented) |
| Cleared by | `Ctrl+S` (save) | user (`M` or `m` to toggle) |
| Purpose | "unsaved work stays visible" | "explicit bookmark" |
| Key column indicator | `#` prefix, yellow | `@` prefix |
| Filter sigil | `#` (narrow to dirty) | TBD |

Both contribute to row visibility independently:
```
visible = matches_filter(key) OR is_dirty(key) OR is_temp_pinned(key) OR is_pinned(key)
```

## ChildrenAll bulk op receipt

When a `[+children all]` rename completes, the hidden children that were renamed
are automatically marked dirty by `rename_key_in_workspace` (which inserts each
new key name into `dirty_keys`).  Because dirty keys bypass the filter, the renamed
entries remain visible without any explicit promotion step.  The user can review
all changes with `/#` in the filter.
