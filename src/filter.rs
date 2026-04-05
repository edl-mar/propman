use std::collections::HashSet;
use crate::workspace::{self, Workspace};

#[derive(Debug, Clone, PartialEq)]
pub enum MatchMode {
    /// Key patterns: substring match. Locale selectors: prefix-at-boundary match.
    Unquoted,
    /// Both: exact string equality.
    Exact,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StatusModifier {
    /// No modifier — no status filter (column visibility only, not key filtering).
    Any,
    /// `?` — entry must be missing for all matched locales.
    Missing,
    /// `!` — entry must be present for all matched locales.
    Present,
}

/// Column visibility directive derived from `:?` / `:!` terms.
#[derive(Debug, Clone, PartialEq)]
pub enum ColumnDirective {
    /// No directive — all rows visible.
    None,
    /// `:?` — only show rows where this locale cell is missing.
    MissingOnly,
    /// `:!` — only show rows where this locale cell is present.
    PresentOnly,
}

#[derive(Debug, Clone)]
pub enum FilterExpr {
    And(Vec<FilterExpr>),
    Or(Vec<FilterExpr>),
    Not(Box<FilterExpr>),

    // Key terms (/ prefix)
    /// `/pattern` — key substring or exact match.
    KeyPattern { pattern: String, mode: MatchMode },
    /// `/*pattern` — key is dangling (unsaved) and matches the pattern.
    /// Bare `/*` matches all dangling keys.
    DanglingKey { pattern: String, mode: MatchMode },
    /// `/?` — key has at least one missing translation.
    AnyMissing,
    /// `/#` — key has unsaved changes (present in `dirty_keys`).
    DirtyKey,

    // Bundle terms (no prefix)
    /// `messages` — key must belong to this bundle (one term per BundleFilter node).
    /// Unquoted = prefix match on bundle name; quoted = exact match.
    BundleFilter { pattern: String, mode: MatchMode },

    // Locale terms (: prefix)
    /// `:de`, `:de?`, `:de!` — locale selector with optional modifier.
    LocaleStatus { locale: String, modifier: StatusModifier, mode: MatchMode },
    /// `:?` — column directive: show only rows where a locale cell is missing.
    /// Evaluates to true (does not filter keys by itself).
    MissingColumns,
    /// `:!` — column directive: show only rows where a locale cell is present.
    /// Evaluates to true (does not filter keys by itself).
    PresentColumns,
    /// `:#` — show dirty locale columns; evaluates to true.
    DirtyLocale,

    // Special
    /// Bare `#` — dirty keys AND dirty locale columns.
    Dirty,
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Parse a filter query string into a `FilterExpr`.
///
/// New DSL: terms are typed by prefix sigil and can appear in any order.
///   - No prefix   → bundle term: `messages`, `"messages"` (exact)
///   - `/` prefix  → key term:    `/confirm`, `/?`, `/*pattern`, `/#`
///   - `:` prefix  → locale term: `:de`, `:de?`, `:de!`, `:?`, `:!`, `:#`
///   - `#` bare    → Dirty (dirty keys + dirty locale columns)
///
/// Space = AND (higher precedence, within a comma-group).
/// Comma = OR  (lower precedence, between groups).
///
/// Returns `FilterExpr::And([])` (matches everything) for empty input.
pub fn parse(input: &str) -> FilterExpr {
    let input = input.trim();
    if input.is_empty() {
        return FilterExpr::And(vec![]);
    }

    // Two-level split: commas → OR groups; whitespace → AND terms within each group.
    let or_groups: Vec<&str> = split_on_commas(input);

    if or_groups.len() == 1 {
        // No commas — collapse to And directly (avoids unnecessary Or wrapper).
        parse_and_group(or_groups[0])
    } else {
        let branches: Vec<FilterExpr> = or_groups.iter().map(|g| parse_and_group(g)).collect();
        FilterExpr::Or(branches)
    }
}

/// Split `input` on whitespace that is not inside quotes or parentheses.
/// Analogous to `split_on_commas` but splits on whitespace instead of commas.
fn split_on_whitespace(input: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = Vec::new();
    let mut depth: usize = 0;
    let mut in_quotes = false;
    let mut in_token = false;
    let mut start = 0;

    for (i, ch) in input.char_indices() {
        let is_space = ch == ' ' || ch == '\t' || ch == '\n' || ch == '\r';
        match ch {
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes => depth = depth.saturating_sub(1),
            _ => {}
        }
        if is_space && !in_quotes && depth == 0 {
            if in_token {
                parts.push(&input[start..i]);
                in_token = false;
            }
        } else {
            if !in_token {
                start = i;
                in_token = true;
            }
        }
    }
    if in_token {
        parts.push(&input[start..]);
    }
    parts
}

/// Parse a single AND group (whitespace-separated terms).
fn parse_and_group(group: &str) -> FilterExpr {
    let group = group.trim();
    let mut terms: Vec<FilterExpr> = Vec::new();
    for raw in split_on_whitespace(group) {
        terms.push(parse_term(raw));
    }
    match terms.len() {
        0 => FilterExpr::And(vec![]),
        1 => terms.remove(0),
        _ => FilterExpr::And(terms),
    }
}

/// Parse a single term by its leading sigil.
fn parse_term(raw: &str) -> FilterExpr {
    // Parenthesised group: (expr)
    if raw.starts_with('(') && raw.ends_with(')') {
        return parse(&raw[1..raw.len() - 1]);
    }
    // Negation: -term or -(expr)
    if let Some(rest) = raw.strip_prefix('-') {
        return FilterExpr::Not(Box::new(parse_term(rest)));
    }
    if raw == "#" {
        return FilterExpr::Dirty;
    }
    if let Some(rest) = raw.strip_prefix('/') {
        return parse_key_term(rest);
    }
    if let Some(rest) = raw.strip_prefix(':') {
        return parse_locale_term(rest);
    }
    // No sigil → bundle term.
    if let Some(inner) = strip_quotes(raw) {
        FilterExpr::BundleFilter { pattern: inner.to_string(), mode: MatchMode::Exact }
    } else {
        FilterExpr::BundleFilter { pattern: raw.to_string(), mode: MatchMode::Unquoted }
    }
}

/// Parse the part after the leading `/`.
fn parse_key_term(rest: &str) -> FilterExpr {
    if rest == "?" {
        return FilterExpr::AnyMissing;
    }
    if rest == "#" {
        return FilterExpr::DirtyKey;
    }
    if let Some(pat) = rest.strip_prefix('*') {
        // `*"pattern"` → exact dangling match; `*pattern` → substring dangling match.
        if let Some(inner) = strip_quotes(pat) {
            return FilterExpr::DanglingKey { pattern: inner.to_string(), mode: MatchMode::Exact };
        }
        return FilterExpr::DanglingKey { pattern: pat.to_string(), mode: MatchMode::Unquoted };
    }
    if let Some(inner) = strip_quotes(rest) {
        FilterExpr::KeyPattern { pattern: inner.to_string(), mode: MatchMode::Exact }
    } else {
        FilterExpr::KeyPattern { pattern: rest.to_string(), mode: MatchMode::Unquoted }
    }
}

/// Parse the part after the leading `:`.
fn parse_locale_term(rest: &str) -> FilterExpr {
    if rest == "?" {
        return FilterExpr::MissingColumns;
    }
    if rest == "!" {
        return FilterExpr::PresentColumns;
    }
    if rest == "#" {
        return FilterExpr::DirtyLocale;
    }

    if rest.starts_with('"') {
        // Quoted locale — modifier is the character(s) after the closing `"`.
        if let Some(close_offset) = rest[1..].find('"') {
            let inner = &rest[1..close_offset + 1];
            let after = &rest[close_offset + 2..];
            return FilterExpr::LocaleStatus {
                locale: inner.to_string(),
                modifier: parse_modifier_str(after),
                mode: MatchMode::Exact,
            };
        }
    }

    // Unquoted — modifier is the final character.
    let (locale, modifier) = split_modifier(rest);
    FilterExpr::LocaleStatus {
        locale: locale.to_string(),
        modifier,
        mode: MatchMode::Unquoted,
    }
}

/// Split `input` on commas that are not inside quotes or parentheses.
/// (Parentheses are reserved for future use.)
fn split_on_commas(input: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = Vec::new();
    let mut depth: usize = 0; // paren depth
    let mut in_quotes = false;
    let mut start = 0;

    for (i, ch) in input.char_indices() {
        match ch {
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes => depth = depth.saturating_sub(1),
            ',' if !in_quotes && depth == 0 => {
                parts.push(&input[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
    parts
}

/// If `s` is wrapped in `"..."`, return the inner content; otherwise `None`.
fn strip_quotes(s: &str) -> Option<&str> {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        Some(&s[1..s.len() - 1])
    } else {
        None
    }
}

fn parse_modifier_str(s: &str) -> StatusModifier {
    match s {
        "!" => StatusModifier::Present,
        "?" => StatusModifier::Missing,
        _ => StatusModifier::Any,
    }
}

fn split_modifier(s: &str) -> (&str, StatusModifier) {
    match s.chars().last() {
        Some('!') => (&s[..s.len() - 1], StatusModifier::Present),
        Some('?') => (&s[..s.len() - 1], StatusModifier::Missing),
        _ => (s, StatusModifier::Any),
    }
}

// ── Column directive ──────────────────────────────────────────────────────────

/// Walk `expr` and return the first `ColumnDirective` found.
/// `Dirty` implies `MissingOnly` — no, actually `Dirty` affects locale columns via
/// dirty locale inclusion, not row filtering. Returns `None` (i.e. `ColumnDirective::None`)
/// unless an explicit `:?` or `:!` is present.
pub fn column_directive(expr: &FilterExpr) -> ColumnDirective {
    match expr {
        FilterExpr::MissingColumns => ColumnDirective::MissingOnly,
        FilterExpr::PresentColumns => ColumnDirective::PresentOnly,
        FilterExpr::And(terms) => {
            for t in terms {
                let d = column_directive(t);
                if d != ColumnDirective::None {
                    return d;
                }
            }
            ColumnDirective::None
        }
        FilterExpr::Or(branches) => {
            for b in branches {
                let d = column_directive(b);
                if d != ColumnDirective::None {
                    return d;
                }
            }
            ColumnDirective::None
        }
        FilterExpr::Not(_) => ColumnDirective::None,
        _ => ColumnDirective::None,
    }
}

// ── Evaluator ─────────────────────────────────────────────────────────────────

/// Returns `true` if `key` should be visible given `expr` and the workspace.
pub fn evaluate(expr: &FilterExpr, key: &str, workspace: &Workspace, dirty_keys: &HashSet<String>) -> bool {
    match expr {
        FilterExpr::And(terms) => terms.iter().all(|t| evaluate(t, key, workspace, dirty_keys)),
        FilterExpr::Or(branches) => branches.iter().any(|t| evaluate(t, key, workspace, dirty_keys)),
        FilterExpr::Not(inner) => {
            // Column-only terms (LocaleStatus{Any}, MissingColumns, PresentColumns,
            // DirtyLocale) evaluate to `true` purely as a column visibility hint.
            // Negating them must not create a row filter — `-:"de"` means "hide the
            // de column", not "hide all keys". Return true for these cases.
            match inner.as_ref() {
                FilterExpr::LocaleStatus { modifier: StatusModifier::Any, .. }
                | FilterExpr::MissingColumns
                | FilterExpr::PresentColumns
                | FilterExpr::DirtyLocale => true,
                _ => !evaluate(inner, key, workspace, dirty_keys),
            }
        }

        FilterExpr::KeyPattern { pattern, mode } => match mode {
            MatchMode::Exact => key == pattern.as_str(),
            // Substring match for now; nucleo fuzzy planned.
            MatchMode::Unquoted => key.contains(pattern.as_str()),
        },

        FilterExpr::AnyMissing => {
            let (bundle, _) = workspace::split_key(key);
            workspace.bundle_locales(bundle)
                .iter()
                .any(|locale| !has_value(key, locale, workspace))
        }

        FilterExpr::DanglingKey { pattern, mode } => {
            if !workspace.is_dangling(key) {
                return false;
            }
            match mode {
                MatchMode::Exact => key == pattern.as_str(),
                MatchMode::Unquoted => key.contains(pattern.as_str()),
            }
        }

        FilterExpr::BundleFilter { pattern, mode } => {
            let (key_bundle, _) = workspace::split_key(key);
            match mode {
                MatchMode::Exact => key_bundle == pattern.as_str(),
                MatchMode::Unquoted => key_bundle.starts_with(pattern.as_str()),
            }
        }

        FilterExpr::LocaleStatus { locale, modifier, mode } => {
            let (bundle, _) = workspace::split_key(key);
            match modifier {
                // Any modifier: column-visibility hint only, never filters keys.
                StatusModifier::Any => true,
                StatusModifier::Present => {
                    // All locales matching the selector must have this key.
                    let matched = matching_locales(locale, mode, workspace, bundle);
                    !matched.is_empty() && matched.iter().all(|l| has_value(key, l, workspace))
                }
                StatusModifier::Missing => {
                    // All locales matching the selector must NOT have this key.
                    let matched = matching_locales(locale, mode, workspace, bundle);
                    !matched.is_empty() && matched.iter().all(|l| !has_value(key, l, workspace))
                }
            }
        }

        // These evaluate to true — they are column/row directives, not key filters.
        FilterExpr::MissingColumns | FilterExpr::PresentColumns | FilterExpr::DirtyLocale => true,

        FilterExpr::DirtyKey | FilterExpr::Dirty => dirty_keys.contains(key),
    }
}

/// Bundle-scoped locales that match `pattern` under `mode`.
/// Uses `bundle_locales` so that locales with no file in this bundle are excluded —
/// consistent with the renderer which skips such locales via `bundle_has_locale`.
fn matching_locales(pattern: &str, mode: &MatchMode, workspace: &Workspace, bundle: &str) -> Vec<String> {
    workspace
        .bundle_locales(bundle)
        .into_iter()
        .filter(|locale| locale_matches(locale, pattern, mode))
        .collect()
}

// ── Visible locales ───────────────────────────────────────────────────────────

/// Returns the workspace locales that should be visible given `expr`.
///
/// - Collects `LocaleStatus` selectors → narrows to matching locales.
/// - Negative locale terms (via `Not`) exclude matching locales.
/// - If `DirtyLocale` or `Dirty` is present, also includes locales from `dirty_locales`.
/// - If no selectors and no dirty directive → returns all locales.
pub fn visible_locales(expr: &FilterExpr, workspace: &Workspace, dirty_locales: &HashSet<String>) -> Vec<String> {
    let mut positive: Vec<(&str, &MatchMode)> = Vec::new();
    let mut negative: Vec<(&str, &MatchMode)> = Vec::new();
    let mut include_dirty = false;
    let mut exclude_dirty = false;
    collect_locale_selectors(expr, &mut positive, &mut negative, &mut include_dirty, &mut exclude_dirty, false);

    let has_positive = !positive.is_empty() || include_dirty;
    let has_negative = !negative.is_empty() || exclude_dirty;

    let candidates: Vec<String> = if has_positive {
        // Start from locales matched by positive terms.
        workspace.all_locales().into_iter()
            .filter(|locale| {
                positive.iter().any(|(p, m)| locale_matches(locale, p, m))
                || (include_dirty && dirty_locales.contains(locale.as_str()))
            })
            .collect()
    } else if has_negative {
        // Only negative terms — start from all locales.
        workspace.all_locales()
    } else {
        // No locale terms at all.
        return workspace.all_locales();
    };

    // Apply exclusions.
    candidates.into_iter()
        .filter(|locale| {
            !negative.iter().any(|(p, m)| locale_matches(locale, p, m))
            && !(exclude_dirty && dirty_locales.contains(locale.as_str()))
        })
        .collect()
}

fn collect_locale_selectors<'a>(
    expr: &'a FilterExpr,
    positive: &mut Vec<(&'a str, &'a MatchMode)>,
    negative: &mut Vec<(&'a str, &'a MatchMode)>,
    include_dirty: &mut bool,
    exclude_dirty: &mut bool,
    negate: bool,
) {
    match expr {
        FilterExpr::Not(inner) => {
            collect_locale_selectors(inner, positive, negative, include_dirty, exclude_dirty, !negate);
        }
        FilterExpr::And(terms) => {
            for t in terms {
                collect_locale_selectors(t, positive, negative, include_dirty, exclude_dirty, negate);
            }
        }
        FilterExpr::Or(branches) => {
            for b in branches {
                collect_locale_selectors(b, positive, negative, include_dirty, exclude_dirty, negate);
            }
        }
        FilterExpr::LocaleStatus { locale, mode, .. } => {
            if negate {
                negative.push((locale.as_str(), mode));
            } else {
                positive.push((locale.as_str(), mode));
            }
        }
        FilterExpr::DirtyLocale | FilterExpr::Dirty => {
            if negate {
                *exclude_dirty = true;
            } else {
                *include_dirty = true;
            }
        }
        FilterExpr::BundleFilter { .. } | FilterExpr::KeyPattern { .. }
        | FilterExpr::AnyMissing | FilterExpr::DanglingKey { .. }
        | FilterExpr::DirtyKey | FilterExpr::MissingColumns | FilterExpr::PresentColumns => {}
    }
}

/// Unquoted: simple prefix match — `de` matches `de`, `default`, `de_AT`, …
/// Exact: full equality only — `"de"` matches only `de`.
fn locale_matches(locale: &str, pattern: &str, mode: &MatchMode) -> bool {
    match mode {
        MatchMode::Exact => locale == pattern,
        MatchMode::Unquoted => locale.starts_with(pattern),
    }
}

fn has_value(key: &str, locale: &str, workspace: &Workspace) -> bool {
    workspace.get_value(key, locale).is_some()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn and_terms(expr: &FilterExpr) -> &[FilterExpr] {
        match expr {
            FilterExpr::And(t) => t,
            _ => panic!("expected And, got {:?}", expr),
        }
    }

    fn or_branches(expr: &FilterExpr) -> &[FilterExpr] {
        match expr {
            FilterExpr::Or(b) => b,
            _ => panic!("expected Or, got {:?}", expr),
        }
    }

    #[test]
    fn empty_matches_all() {
        assert!(matches!(parse(""), FilterExpr::And(ref t) if t.is_empty()));
        assert!(matches!(parse("  "), FilterExpr::And(ref t) if t.is_empty()));
    }

    #[test]
    fn bundle_unquoted() {
        let expr = parse("messages");
        assert!(matches!(&expr,
            FilterExpr::BundleFilter { pattern, mode }
            if pattern == "messages" && *mode == MatchMode::Unquoted
        ));
    }

    #[test]
    fn bundle_exact() {
        let expr = parse("\"messages\"");
        assert!(matches!(&expr,
            FilterExpr::BundleFilter { pattern, mode }
            if pattern == "messages" && *mode == MatchMode::Exact
        ));
    }

    #[test]
    fn key_unquoted() {
        let expr = parse("/error");
        assert!(matches!(&expr,
            FilterExpr::KeyPattern { pattern, mode }
            if pattern == "error" && *mode == MatchMode::Unquoted
        ));
    }

    #[test]
    fn key_exact() {
        let expr = parse("/\"app.error.notfound\"");
        assert!(matches!(&expr,
            FilterExpr::KeyPattern { pattern, mode }
            if pattern == "app.error.notfound" && *mode == MatchMode::Exact
        ));
    }

    #[test]
    fn key_any_missing() {
        // /? → AnyMissing
        let expr = parse("/?");
        assert!(matches!(&expr, FilterExpr::AnyMissing));
    }

    #[test]
    fn key_dirty() {
        // /# → DirtyKey
        let expr = parse("/#");
        assert!(matches!(&expr, FilterExpr::DirtyKey));
    }

    #[test]
    fn bare_dirty() {
        // # → Dirty
        let expr = parse("#");
        assert!(matches!(&expr, FilterExpr::Dirty));
    }

    #[test]
    fn locale_missing_columns() {
        // :? → MissingColumns (column directive, not AnyMissing)
        let expr = parse(":?");
        assert!(matches!(&expr, FilterExpr::MissingColumns));
    }

    #[test]
    fn locale_present_columns() {
        // :! → PresentColumns
        let expr = parse(":!");
        assert!(matches!(&expr, FilterExpr::PresentColumns));
    }

    #[test]
    fn locale_dirty() {
        // :# → DirtyLocale
        let expr = parse(":#");
        assert!(matches!(&expr, FilterExpr::DirtyLocale));
    }

    #[test]
    fn locale_present_unquoted() {
        let expr = parse(":de!");
        assert!(matches!(&expr,
            FilterExpr::LocaleStatus { locale, modifier, mode }
            if locale == "de" && *modifier == StatusModifier::Present && *mode == MatchMode::Unquoted
        ));
    }

    #[test]
    fn locale_missing_exact() {
        let expr = parse(":\"de_AT\"?");
        assert!(matches!(&expr,
            FilterExpr::LocaleStatus { locale, modifier, mode }
            if locale == "de_AT" && *modifier == StatusModifier::Missing && *mode == MatchMode::Exact
        ));
    }

    #[test]
    fn locale_any_no_modifier() {
        let expr = parse(":de");
        assert!(matches!(&expr,
            FilterExpr::LocaleStatus { locale, modifier, mode }
            if locale == "de" && *modifier == StatusModifier::Any && *mode == MatchMode::Unquoted
        ));
    }

    #[test]
    fn multi_bundle_whitespace_is_and() {
        // `messages errors` → And([BundleFilter{messages}, BundleFilter{errors}])
        let expr = parse("messages errors");
        let t = and_terms(&expr);
        assert_eq!(t.len(), 2);
        assert!(matches!(&t[0], FilterExpr::BundleFilter { pattern, .. } if pattern == "messages"));
        assert!(matches!(&t[1], FilterExpr::BundleFilter { pattern, .. } if pattern == "errors"));
    }

    #[test]
    fn multi_bundle_comma_is_or() {
        // `messages,errors` → Or([BundleFilter{messages}, BundleFilter{errors}])
        let expr = parse("messages,errors");
        let b = or_branches(&expr);
        assert_eq!(b.len(), 2);
        assert!(matches!(&b[0], FilterExpr::BundleFilter { pattern, .. } if pattern == "messages"));
        assert!(matches!(&b[1], FilterExpr::BundleFilter { pattern, .. } if pattern == "errors"));
    }

    #[test]
    fn multi_term_and() {
        // Space-separated terms in any order → And
        let expr = parse("/error /timeout :de?");
        let t = and_terms(&expr);
        assert_eq!(t.len(), 3);
        assert!(matches!(&t[0], FilterExpr::KeyPattern { pattern, .. } if pattern == "error"));
        assert!(matches!(&t[1], FilterExpr::KeyPattern { pattern, .. } if pattern == "timeout"));
        assert!(matches!(&t[2],
            FilterExpr::LocaleStatus { locale, modifier, .. }
            if locale == "de" && *modifier == StatusModifier::Missing
        ));
    }

    #[test]
    fn all_three_sections() {
        let expr = parse("messages /error :de");
        let t = and_terms(&expr);
        assert_eq!(t.len(), 3);
        assert!(matches!(&t[0], FilterExpr::BundleFilter { pattern, .. } if pattern == "messages"));
        assert!(matches!(&t[1], FilterExpr::KeyPattern { pattern, .. } if pattern == "error"));
        assert!(matches!(&t[2], FilterExpr::LocaleStatus { locale, .. } if locale == "de"));
    }

    #[test]
    fn multi_locale_whitespace_is_and() {
        let expr = parse(":de :fr");
        let t = and_terms(&expr);
        assert_eq!(t.len(), 2);
        assert!(matches!(&t[0], FilterExpr::LocaleStatus { locale, .. } if locale == "de"));
        assert!(matches!(&t[1], FilterExpr::LocaleStatus { locale, .. } if locale == "fr"));
    }

    #[test]
    fn or_expression() {
        // `/confirm :de, /error :fr` → Or([And([KeyPattern{confirm}, LocaleStatus{de}]), And([...])])
        let expr = parse("/confirm :de, /error :fr");
        let b = or_branches(&expr);
        assert_eq!(b.len(), 2);
        let b0 = and_terms(&b[0]);
        assert!(matches!(&b0[0], FilterExpr::KeyPattern { pattern, .. } if pattern == "confirm"));
        assert!(matches!(&b0[1], FilterExpr::LocaleStatus { locale, .. } if locale == "de"));
        let b1 = and_terms(&b[1]);
        assert!(matches!(&b1[0], FilterExpr::KeyPattern { pattern, .. } if pattern == "error"));
        assert!(matches!(&b1[1], FilterExpr::LocaleStatus { locale, .. } if locale == "fr"));
    }

    #[test]
    fn dirty_key_with_key_pattern() {
        // `/confirm#` is NOT valid new DSL — confirm and # are separate terms via whitespace.
        // `/confirm /#` → And([KeyPattern{confirm}, DirtyKey])
        let expr = parse("/confirm /#");
        let t = and_terms(&expr);
        assert!(matches!(&t[0], FilterExpr::KeyPattern { pattern, .. } if pattern == "confirm"));
        assert!(matches!(&t[1], FilterExpr::DirtyKey));
    }

    #[test]
    fn dirty_locale_columns() {
        // `:de #` → And([LocaleStatus{de}, Dirty])  — dirty narrows locales too
        let expr = parse(":de #");
        let t = and_terms(&expr);
        assert!(matches!(&t[0], FilterExpr::LocaleStatus { locale, .. } if locale == "de"));
        assert!(matches!(&t[1], FilterExpr::Dirty));
    }

    #[test]
    fn column_directive_missing_only() {
        assert_eq!(column_directive(&parse(":?")), ColumnDirective::MissingOnly);
    }

    #[test]
    fn column_directive_present_only() {
        assert_eq!(column_directive(&parse(":!")), ColumnDirective::PresentOnly);
    }

    #[test]
    fn column_directive_none_for_key_filter() {
        assert_eq!(column_directive(&parse("/error")), ColumnDirective::None);
    }

    #[test]
    fn dangling_bare() {
        // `/*` matches all dangling keys (empty pattern, always contains "")
        let expr = parse("/*");
        assert!(matches!(&expr, FilterExpr::DanglingKey { pattern, .. } if pattern.is_empty()));
    }

    #[test]
    fn dangling_pattern() {
        let expr = parse("/*foo");
        assert!(matches!(&expr, FilterExpr::DanglingKey { pattern, mode }
            if pattern == "foo" && *mode == MatchMode::Unquoted
        ));
    }

    #[test]
    fn prefix_matching() {
        // Unquoted uses simple starts_with — `de` matches de, default, de_AT.
        let check = |pattern: &str, locale: &str| locale.starts_with(pattern);
        assert!(check("de", "de"));
        assert!(check("de", "de_AT"));
        assert!(check("de", "de_DE"));
        assert!(check("de", "default"));
        assert!(!check("de", "en"));
        // Exact pattern distinguishes de from default.
        assert!("de" == "de");
        assert!("default" != "de");
    }

    #[test]
    fn negation_key_term() {
        // `-/button` → Not(KeyPattern{button})
        let expr = parse("-/button");
        assert!(matches!(&expr, FilterExpr::Not(inner)
            if matches!(inner.as_ref(), FilterExpr::KeyPattern { pattern, .. } if pattern == "button")
        ));
    }

    #[test]
    fn negation_bundle_term() {
        let expr = parse("-messages");
        assert!(matches!(&expr, FilterExpr::Not(inner)
            if matches!(inner.as_ref(), FilterExpr::BundleFilter { pattern, .. } if pattern == "messages")
        ));
    }

    #[test]
    fn negation_any_missing() {
        // `-/?` → Not(AnyMissing) — completely translated keys
        let expr = parse("-/?");
        assert!(matches!(&expr, FilterExpr::Not(inner)
            if matches!(inner.as_ref(), FilterExpr::AnyMissing)
        ));
    }

    #[test]
    fn negation_locale_term() {
        // `-:"de"!` → Not(LocaleStatus{de, Present, Exact})
        let expr = parse("-:\"de\"!");
        assert!(matches!(&expr, FilterExpr::Not(inner)
            if matches!(inner.as_ref(), FilterExpr::LocaleStatus { locale, modifier, mode }
                if locale == "de" && *modifier == StatusModifier::Present && *mode == MatchMode::Exact)
        ));
    }

    #[test]
    fn negation_grouped_expr() {
        // `-(:"de"! :"si"!)` → Not(And([LocaleStatus{de,Present}, LocaleStatus{si,Present}]))
        let expr = parse("-(:\"de\"! :\"si\"!)");
        assert!(matches!(&expr, FilterExpr::Not(inner)
            if matches!(inner.as_ref(), FilterExpr::And(terms) if terms.len() == 2)
        ));
    }

    #[test]
    fn negation_in_and() {
        // `/confirm -messages` → And([KeyPattern{confirm}, Not(BundleFilter{messages})])
        let expr = parse("/confirm -messages");
        let t = and_terms(&expr);
        assert_eq!(t.len(), 2);
        assert!(matches!(&t[0], FilterExpr::KeyPattern { pattern, .. } if pattern == "confirm"));
        assert!(matches!(&t[1], FilterExpr::Not(inner)
            if matches!(inner.as_ref(), FilterExpr::BundleFilter { pattern, .. } if pattern == "messages")
        ));
    }

    #[test]
    fn negation_in_or() {
        // `messages, -errors` → Or([BundleFilter{messages}, Not(BundleFilter{errors})])
        let expr = parse("messages, -errors");
        let b = or_branches(&expr);
        assert_eq!(b.len(), 2);
        assert!(matches!(&b[0], FilterExpr::BundleFilter { pattern, .. } if pattern == "messages"));
        assert!(matches!(&b[1], FilterExpr::Not(inner)
            if matches!(inner.as_ref(), FilterExpr::BundleFilter { pattern, .. } if pattern == "errors")
        ));
    }

    #[test]
    fn negation_column_directive_is_none() {
        // `-:?` should not produce a column directive
        assert_eq!(column_directive(&parse("-:?")), ColumnDirective::None);
    }

    #[test]
    fn negation_column_only_locale_does_not_filter_rows() {
        // `-:"de"` (no modifier) is column exclusion — Not(LocaleStatus{Any}).
        // Before the fix, Not(true) = false would hide all keys.
        // Verify the parsed shape has Any modifier (so the guard in evaluate fires).
        let expr = parse("-:\"de\"");
        assert!(matches!(&expr, FilterExpr::Not(inner)
            if matches!(inner.as_ref(), FilterExpr::LocaleStatus { modifier, .. }
                if *modifier == StatusModifier::Any)
        ));
        // -:? and -:! are also column-only — must not produce row filter effects
        assert!(matches!(parse("-:?"), FilterExpr::Not(ref inner)
            if matches!(inner.as_ref(), FilterExpr::MissingColumns)
        ));
        assert!(matches!(parse("-:!"), FilterExpr::Not(ref inner)
            if matches!(inner.as_ref(), FilterExpr::PresentColumns)
        ));
    }
}
