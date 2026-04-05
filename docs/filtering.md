# Filtering

The filter bar (press `/`) accepts a boolean expression over typed terms.
Each term is self-describing — the sigil prefix determines what it matches.
Terms can appear in any order.

## Syntax overview

```
term      = bundle_term | key_term | locale_term | dirty_term
bundle_term = messages              (no prefix — prefix match on bundle name)
            | "messages"            (quoted — exact match)
key_term  = /confirm                (/ prefix — substring match on key)
          | /"confirm"              (/ prefix — exact match)
          | /?                      (/ prefix — key has any missing translation)
          | /*pattern               (/ prefix — dangling/unsaved key)
          | /#                      (/ prefix — dirty keys only)
locale_term = :de                   (: prefix — show de column)
            | :"de"                 (: prefix — exact locale name)
            | :de?                  (: prefix — missing in de + show de column)
            | :de!                  (: prefix — present in de + show de column)
            | :?                    (: prefix — per row: show only missing locale columns)
            | :!                    (: prefix — per row: show only present locale columns)
            | :#                    (: prefix — show only dirty locale columns)
dirty_term  = #                     (bare — dirty keys + dirty locale columns)

operators:
  space    AND  (higher precedence)
  ,        OR   (lower precedence)
  ( )      grouping
```

## Match modes

**Bundle terms** use prefix matching by default:
```
messages        — bundle name starts with "messages" (matches messages, messages_old, …)
"messages"      — exact: bundle named exactly "messages"
```

**Key terms** use substring matching by default:
```
/confirm        — key contains "confirm"
/"confirm"      — key equals "confirm" exactly
```

**Locale terms** use prefix matching by default:
```
:de             — locale starts with "de" (matches de, de_AT, de_DE, default, …)
:"de"           — exact: locale named exactly "de"
```

Use quotes to avoid over-matching (e.g. `:"de"` to exclude `default`).

## Boolean operators

`space` binds tighter than `,`:

```
messages /confirm         — bundle starts with "messages" AND key contains "confirm"
messages /confirm, errors /delete
                          — (messages AND confirm) OR (errors AND delete)
(messages, errors) /confirm
                          — (messages OR errors) AND confirm
```

## Key terms

```
/error          — keys containing "error"
/"app.title"    — key named exactly "app.title"
/?              — keys with at least one missing translation (find gaps)
                  "missing" = bundle has a locale file but key has no entry in it;
                  locales with no file in the bundle are not considered
/*              — all dangling (unsaved, not yet in any file) keys
/*pattern       — dangling keys containing "pattern"
/#              — dirty keys only (row filter, all locale columns shown)
```

## Bundle terms

```
messages        — keys in bundles starting with "messages"
errors          — keys in bundles starting with "errors"
messages,errors — keys in either bundle (OR)
"messages"      — keys in the bundle named exactly "messages"
```

## Locale terms

```
:de             — show de* columns (no row filter)
:"de"           — show exactly the de column (no row filter)
:de?            — keys missing in all de* locales; show de* columns
:de!            — keys present in all de* locales; show de* columns
:"de"?          — keys missing in exactly de; show de column
:"de"!          — keys present in exactly de; show de column
```

### Column directives

These affect which locale columns are rendered per row, but never filter rows:

```
:?              — per row: show only the locale columns where this key is missing
:!              — per row: show only the locale columns where this key is present
:#              — globally: show only dirty locale columns
```

### Locale modifiers summary

| suffix | meaning |
|--------|---------|
| (none) | show column only, no row filter |
| `?`    | all matched locales must be missing |
| `!`    | all matched locales must be present |

## The `#` dirty shorthand

```
#               — dirty keys + narrow to dirty locale columns
/#              — dirty keys only (all locale columns shown)
:#              — all keys, but show only dirty locale columns
# :de           — dirty keys + dirty locale columns + de column
# messages      — dirty keys in messages bundle, dirty locale columns
```

`#` is a reserved token — it does not follow the no-prefix = bundle rule.

Dirty keys also **bypass the filter** automatically: they remain visible
regardless of any other filter terms while unsaved changes are pending.

## Column visibility rules

1. Any named locale term (`:de`, `:de?`, `:de!`, `:"de"`, …) adds that locale to
   the global visible column set.
2. `:#` or `#` adds the dirty locale columns to the visible set.
3. `:?` and `:!` are per-row directives and do not affect the global column set.
4. When no locale terms are present, all locale columns are shown.

## Examples

```
/?
    keys with any missing translation

:de?
    keys missing in all de* locales; show de* columns

:"de"?
    keys missing in exactly [de]; show de column

:"de"! :"si"!
    [de] present AND [si] present

/error :de
    keys containing "error", show de* columns only

/notfound /timeout :"default"! :"de"?
    keys matching both "notfound" and "timeout",
    where [default] is present AND [de] is missing

:de?, :si?
    keys missing in de* OR keys missing in si*

messages /error :de
    keys containing "error" in the messages bundle, narrowed to de* columns

messages /confirm, errors /delete :de?
    (confirm keys in messages bundle)
    OR (delete keys in errors bundle that are missing in de)

#
    all dirty keys, show dirty locale columns only

# :de
    dirty keys, show dirty locale columns and de alongside

/?  :?
    keys with any missing translation; per row show only the missing columns

/?  :!
    keys with any missing translation; per row show only the present columns
```

## Internal representation

The filter input is parsed into a `FilterExpr` AST.

```rust
enum ColumnDirective { None, MissingOnly, PresentOnly }

enum FilterExpr {
    And(Vec<FilterExpr>),
    Or(Vec<FilterExpr>),
    // Key terms
    KeyPattern { pattern: String, mode: MatchMode },
    DanglingKey { pattern: String, mode: MatchMode },
    AnyMissing,   // /?
    DirtyKey,     // /#
    // Bundle terms
    BundleFilter { pattern: String, mode: MatchMode },
    // Locale terms
    LocaleStatus { locale: String, modifier: StatusModifier, mode: MatchMode },
    MissingColumns,  // :?
    PresentColumns,  // :!
    DirtyLocale,     // :#
    // Special
    Dirty,           // bare #
}
```

**Parsing**: two-level split. Commas produce `Or` branches; whitespace within
each branch produces `And` terms. Each term is parsed by its leading sigil.

**Evaluation** (`evaluate(expr, key, workspace, dirty_keys) -> bool`):
- `And` — all terms must match
- `Or` — at least one term must match
- `KeyPattern` — substring or exact match
- `AnyMissing` — at least one of the key's bundle locales is missing this key
- `BundleFilter` — key's bundle matches prefix or exact
- `LocaleStatus { Any }` — always true (column visibility only)
- `LocaleStatus { Present }` — all matched locales must have the key
- `LocaleStatus { Missing }` — all matched locales must not have the key
- `DirtyKey` / `Dirty` — key is in `dirty_keys`
- `MissingColumns` / `PresentColumns` / `DirtyLocale` — always true (column directives)

**Column visibility** (`visible_locales(expr, workspace, dirty_locales) -> Vec<String>`):
Collects all named locale selectors from the expression tree (including `Or`
branches) and returns matching workspace locales. When `Dirty` or `DirtyLocale`
is present, dirty locale columns are added. When no selectors are found, all
locales are returned.

**Per-row directives** (`column_directive(expr) -> ColumnDirective`):
Walks the tree for `MissingColumns` or `PresentColumns`. The renderer uses
this to skip cells per row: `MissingOnly` hides present cells, `PresentOnly`
hides missing cells.
