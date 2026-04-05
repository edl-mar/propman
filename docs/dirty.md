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

`#` is the dirty sigil.  Three forms are available:

| Filter expression | Meaning |
|---|---|
| `#` | dirty keys; show only dirty locale columns |
| `/#` | dirty keys only (row filter; all locale columns shown) |
| `:#` | all keys; show only dirty locale columns |
| `# :de` | dirty keys; dirty locale columns + de column |
| `# messages` | dirty keys in messages bundle; dirty locale columns |
| `/# :de?` | dirty keys that are also missing in de |

`#` is a **reserved token** — it does not follow the no-prefix = bundle rule.
Bare `#` is the quickest way to review all unsaved changes: it combines the
`/#` row filter with the `:#` column narrowing in one character.

Filter terms can be combined with AND (space) and OR (comma).  For example,
`/# :de?` = dirty AND missing in de; `/#, :de?` = dirty OR missing in de.

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
