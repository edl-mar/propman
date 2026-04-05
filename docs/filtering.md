# Filtering

The filter bar (press `/`) accepts a boolean expression over typed terms.
Each term is self-describing — the sigil prefix determines what it matches.
Terms can appear in any order.

## Syntax overview

```
term      = bundle_term | key_term | locale_term | value_term | dirty_term
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
            | :de#                  (: prefix — dirty in de + show de column)
            | :?                    (: prefix — per row: show only missing locale columns)
            | :!                    (: prefix — per row: show only present locale columns)
            | :#                    (: prefix — show only dirty locale columns)
value_term  = =confirm              (= prefix — any locale value contains "confirm", case-insensitive)
            | ="Confirm deletion"   (= prefix — quoted for multi-word patterns)
dirty_term  = #                     (bare — dirty keys + dirty locale columns)

negation:
  -term     negate any term or group: -/button  -messages  -:"de"!  -(expr)

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

## Negation

A `-` prefix negates any term or parenthesised group:

```
-/button            — hide keys containing "button"
-messages           — hide the messages bundle
-/?                 — keys with NO missing translation (completely translated)
-:"de"!             — keys NOT present in de (equivalent to :"de"? for exact match)
-:"de"#             — keys NOT dirty in de
-:#                 — show all locale columns EXCEPT dirty ones
-(:"de"! :"si"!)    — hide keys that have both de and si translations
```

**Negation and column visibility**: `-:de` alone (no positive locale terms) starts
from all locales and removes de* — so `-:"de"` = all locales except de.
When positive and negative locale terms are mixed, the positive terms define
the initial set and the negative terms subtract from it.

**Tautology**: `:de, -:de` = `X OR NOT(X)` = always true = no locale restriction
= all columns shown. The system collapses consistently.

**Note on `?` vs `!` redundancy**: for exact locale matches, `:"de"?` and `-:"de"!`
are equivalent, as are `:"de"!` and `-:"de"?`. Both forms are kept because the
positive shorthands (`:"de"!`, `:"de"?`) are more readable than double negatives.
For prefix matches the "all must" semantics means they are not exact negations of
each other.

## Key terms

```
/error          — keys containing "error"
/"app.title"    — key named exactly "app.title"
/?              — keys with at least one missing translation (find gaps)
                  "missing" = bundle has a locale file but key has no entry in it;
                  locales with no file in the bundle are not considered
-/?             — keys with no missing translations (completely translated)
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
-messages       — hide the messages bundle
```

## Locale terms

```
:de             — show de* columns (no row filter)
:"de"           — show exactly the de column (no row filter)
-:"de"          — hide the de column; alone = all columns except de
:de?            — keys missing in all de* locales; show de* columns
:de!            — keys present in all de* locales; show de* columns
:de#            — keys with dirty cells in de*; show de* columns
:"de"?          — keys missing in exactly de; show de column
:"de"!          — keys present in exactly de; show de column
:"de"#          — keys with a dirty de entry; show de column
-:"de"?         — keys NOT missing in de (present or bundle has no de file)
-:"de"!         — keys NOT present in de (missing)
-:"de"#         — keys with no dirty de entry
```

### Column directives

These affect which locale columns are rendered per row, but never filter rows:

```
:?              — per row: show only the locale columns where this key is missing
:!              — per row: show only the locale columns where this key is present
:#              — globally: show only dirty locale columns
-:#             — globally: show all locale columns except dirty ones
```

### Locale modifiers summary

| suffix | meaning |
|--------|---------|
| (none) | show column only, no row filter |
| `?`    | all matched locales must be missing |
| `!`    | all matched locales must be present |
| `#`    | key has a dirty entry in matched locales |

## Value terms

Value terms match against translation values (not key names).
They are always **case-insensitive substring** matches — there is no exact mode.
Quotes are only needed to include spaces in the pattern:

```
=confirm            — any locale's value contains "confirm"
="Confirm deletion" — any locale's value contains "Confirm deletion"
-=confirm           — no locale's value contains "confirm"
```

Value terms compose with everything else:

```
="delete" ="confirm"        — value contains both "delete" AND "confirm"
="delete", ="confirm"       — value contains "delete" OR "confirm"
="delete" :de               — value contains "delete"; show de column
messages ="delete"          — messages bundle, value contains "delete"
```

**Why `=` and not bare quotes?**
Bare quoted strings (`"messages"`) are already exact bundle matches.
The `=` sigil unambiguously means "search in values".

## The `#` dirty shorthand

```
#               — dirty keys + narrow to dirty locale columns
/#              — dirty keys only (all locale columns shown)
:#              — all keys, but show only dirty locale columns
-:#             — all keys, show all columns except dirty ones
# :de           — dirty keys + dirty locale columns + de column
# messages      — dirty keys in messages bundle, dirty locale columns
```

`#` is a reserved token — it does not follow the no-prefix = bundle rule.

Dirty keys also **bypass the filter** automatically: they remain visible
regardless of any other filter terms while unsaved changes are pending.

## Column visibility rules

1. Any named locale term (`:de`, `:de?`, `:de!`, `:"de"`, …) adds that locale to
   the global visible column set.
2. A negated locale term (`-:"de"`) removes that locale from the visible set.
3. When only negative locale terms are present, the starting set is all locales
   (then exclusions are applied). When positive terms are present, the starting
   set is empty (then inclusions are applied, then exclusions).
4. `:#` or `#` adds dirty locale columns to the visible set; `-:#` removes them.
5. `:?` and `:!` are per-row directives and do not affect the global column set.
6. When no locale terms are present, all locale columns are shown.

## Examples

```
/?
    keys with any missing translation

-/?
    completely translated keys (no missing translations)

:de?
    keys missing in all de* locales; show de* columns

:"de"?
    keys missing in exactly [de]; show de column

:"de"! :"si"!
    [de] present AND [si] present

-:"de"
    all locale columns except de

:de?, :si?
    keys missing in de* OR keys missing in si*

/error :de
    keys containing "error", show de* columns only

/notfound /timeout :"default"! :"de"?
    keys matching both "notfound" and "timeout",
    where [default] is present AND [de] is missing

messages /confirm, errors /delete :de?
    (confirm keys in messages bundle)
    OR (delete keys in errors bundle that are missing in de)

-/button /?
    keys with missing translations, excluding button keys

#
    all dirty keys, show dirty locale columns only

-:#
    all keys, show only the clean (non-dirty) locale columns

# :de
    dirty keys, show dirty locale columns and de alongside

/?  :?
    keys with any missing translation; per row show only the missing columns

/?  :!
    keys with any missing translation; per row show only the present columns

:"de"# -:"si"#
    dirty in de AND not dirty in si

=confirm
    keys where any locale value contains "confirm"

="Confirm deletion"
    keys where any locale value contains "Confirm deletion"

="delete" ="confirm"
    keys where any locale value contains both "delete" and "confirm"

="delete", ="confirm"
    keys where any locale value contains "delete" or "confirm"

="delete" :de
    keys where any locale value contains "delete"; show de column only
```

## Internal representation

The filter input is parsed into a `FilterExpr` AST.

```rust
enum ColumnDirective { None, MissingOnly, PresentOnly }

enum FilterExpr {
    And(Vec<FilterExpr>),
    Or(Vec<FilterExpr>),
    Not(Box<FilterExpr>),
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
    // Value terms
    ValueMatch { pattern: String },  // =pattern — case-insensitive substring, any locale
    // Special
    Dirty,           // bare #
}

enum StatusModifier {
    Any,      // no modifier — column visibility only
    Missing,  // ? — all matched locales must not have the key
    Present,  // ! — all matched locales must have the key
    Dirty,    // # — key has a dirty entry in matched locales (planned)
}
```

**Parsing**: two-level split. Commas produce `Or` branches; whitespace within
each branch produces `And` terms. Each term is parsed by its leading sigil.
A leading `-` wraps the parsed term in `Not(...)`.

**Evaluation** (`evaluate(expr, key, workspace, dirty_keys) -> bool`):
- `And` — all terms must match
- `Or` — at least one term must match
- `Not` — negates the inner expression
- `KeyPattern` — substring or exact match
- `AnyMissing` — at least one of the key's bundle locales is missing this key
- `BundleFilter` — key's bundle matches prefix or exact
- `LocaleStatus { Any }` — always true (column visibility only)
- `LocaleStatus { Present }` — all matched locales must have the key
- `LocaleStatus { Missing }` — all matched locales must not have the key
- `LocaleStatus { Dirty }` — key has a dirty entry in matched locales (planned)
- `DirtyKey` / `Dirty` — key is in `dirty_keys`
- `MissingColumns` / `PresentColumns` / `DirtyLocale` — always true (column directives)
- `ValueMatch` — any locale value for the key's bundle contains the pattern (case-insensitive)

**Column visibility** (`visible_locales(expr, workspace, dirty_locales) -> Vec<String>`):
Collects named locale selectors (positive and negative) from the expression tree.
When only negative selectors exist, starts from all locales and subtracts.
When positive selectors exist, starts from empty, adds matches, then subtracts
negated matches. `Dirty`/`DirtyLocale` adds dirty locale columns; `-:#` removes
them. `:?` and `:!` are per-row directives and do not affect this set.

**Per-row directives** (`column_directive(expr) -> ColumnDirective`):
Walks the tree for `MissingColumns` or `PresentColumns`. The renderer uses
this to skip cells per row: `MissingOnly` hides present cells, `PresentOnly`
hides missing cells.
