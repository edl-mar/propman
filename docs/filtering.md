# Filtering

Filtering is the core feature of propman. The filter system is designed to be
simple in v1 but extensible — the internal representation is a proper AST so
that OR, grouping, and new operators can be added later without touching the
evaluation or UI logic.

## Syntax
```
[key_pattern, ...] : [locale][modifier], ...
```

The `:` is the separator between the key filter and the locale filter.
It is required even when only filtering by locale.

## Match modes

Key patterns and locale selectors use different matching strategies.

**Key patterns** use substring matching (nucleo fuzzy planned):
```
pattern     — substring match against the full key string
"pattern"   — exact match (full key must equal pattern)
```

**Locale selectors** use simple prefix matching:
```
de          — prefix: locale.starts_with("de")
              matches de, default, de_AT, de_DE, de_CH, …
"de"        — exact match: locale == "de" only
```

Use quotes to pin an exact locale code when the prefix would be too broad
(e.g. `"de"` to exclude `default`, or `de_` to match only locale variants).

## Key patterns

Comma-separated patterns matched against the full key string.
Multiple patterns are combined with AND.
```
error                   — substring: matches any key containing "error"
notfound, timeout       — substring AND substring: key must match both
"app.error.notfound"    — exact: matches only this key
```

## Locale selectors

Comma-separated locale identifiers, each with an optional modifier.
Multiple selectors are combined with AND.
```
de!         — prefix: all locales starting with "de" must be present
"de"!       — exact: only [de] itself must be present
"de_AT"?    — exact: only [de_AT] must be missing
de          — prefix: only "de*" columns shown, no status filter
```

### Modifiers
```
(none)  — column is shown, no status filter
!       — entry must be present for all matched locales
?       — entry must be missing for all matched locales
```

### Special selectors
```
?       — any locale is missing (shorthand for the common "find gaps" workflow)
```

## Column visibility

Any `LocaleStatus` term (regardless of modifier) narrows the visible columns
to only the locales that match its selector. When the expression contains no
locale selectors, all columns are visible.

```
:de     — shows only de* columns (de, default, de_AT, …); all keys visible
:de?    — shows only de* columns; filters to keys where ALL de* are missing
:?      — all columns visible; filters to keys where ANY locale is missing
```

## Examples
```
:?
    any translation is missing

:de?
    all locales starting with "de" are missing

:"de"?
    exactly [de] is missing

:"de"!, "si"!
    [de] is present AND [si] is present

error:
    all keys containing "error", all locales visible

notfound, timeout: "default"!, "de"?
    keys matching both "notfound" and "timeout",
    where [default] is present AND [de] is missing

:"de"?, "si"?
    both [de] and [si] are missing
```

## Internal representation

The filter input is parsed into a `FilterExpr` AST before evaluation.
This keeps the parser, evaluator, and UI fully decoupled.
```rust
enum MatchMode {
    Unquoted,  // key: substring match. locale: starts_with match.
    Exact,     // both: full string equality.
}

enum StatusModifier {
    Any,      // no modifier — column visibility only, no key filtering
    Missing,  // ?  — all matched locales must NOT have the key
    Present,  // !  — all matched locales must have the key
}

enum FilterExpr {
    And(Vec<FilterExpr>),
    // Or(Vec<FilterExpr>),  — reserved for later
    KeyPattern { pattern: String, mode: MatchMode },
    LocaleStatus { locale: String, modifier: StatusModifier, mode: MatchMode },
    AnyMissing,  // bare `?` — at least one locale is missing
}
```

Parsing steps:
1. Split input on `:` — left side is key patterns, right side is locale selectors
2. Split each side on `,` and trim whitespace
3. For each term: check for wrapping `"..."` → `MatchMode::Exact`, else `MatchMode::Unquoted`
4. Parse modifier suffix `?` or `!` from locale selectors
5. Parse bare `?` on the right side into `FilterExpr::AnyMissing`
6. Wrap all terms in `FilterExpr::And`

## Evaluation

```rust
fn evaluate(expr: &FilterExpr, key: &str, workspace: &Workspace) -> bool
```

- `And` — short-circuits on the first false term
- `KeyPattern` — substring or exact match against the key string
- `LocaleStatus { Any }` — always true (column visibility only)
- `LocaleStatus { Present }` — all locales matching the selector must have the key
- `LocaleStatus { Missing }` — all locales matching the selector must NOT have the key
- `AnyMissing` — at least one workspace locale does not have the key

```rust
fn visible_locales(expr: &FilterExpr, workspace: &Workspace) -> Vec<String>
```

Collects all `LocaleStatus` selectors from the expression and returns the
workspace locales that match at least one of them. Returns all locales when
the expression contains no locale selectors.

## Relation to Selection and Actions

The filter defines the *visible* space. Selection operates on this filtered
space. Actions are applied to all selected entries — this enables batch
workflows such as:
```
:"de"?      filter all keys where [de] is missing
Shift+A     select all visible entries
Action      copy [default] value to [de] for all selected
```
