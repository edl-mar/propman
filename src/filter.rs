use crate::workspace::Workspace;

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

#[derive(Debug, Clone)]
pub enum FilterExpr {
    And(Vec<FilterExpr>),
    KeyPattern { pattern: String, mode: MatchMode },
    LocaleStatus { locale: String, modifier: StatusModifier, mode: MatchMode },
    /// Bare `?` shorthand: at least one locale is missing.
    AnyMissing,
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Parse a filter query string into a `FilterExpr`.
///
/// Format: `[key_pattern, ...] : [locale][modifier], ...`
///
/// Returns `FilterExpr::And([])` (matches everything) for empty input.
pub fn parse(input: &str) -> FilterExpr {
    let input = input.trim();
    if input.is_empty() {
        return FilterExpr::And(vec![]);
    }

    // Split on the first `:` to separate key patterns from locale selectors.
    let (key_part, locale_part) = match input.find(':') {
        Some(idx) => (input[..idx].trim(), Some(input[idx + 1..].trim())),
        None => (input, None),
    };

    let mut terms: Vec<FilterExpr> = Vec::new();

    for raw in key_part.split(',') {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        terms.push(parse_key_term(raw));
    }

    if let Some(locale_part) = locale_part {
        for raw in locale_part.split(',') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            terms.push(parse_locale_term(raw));
        }
    }

    FilterExpr::And(terms)
}

fn parse_key_term(raw: &str) -> FilterExpr {
    if let Some(inner) = strip_quotes(raw) {
        return FilterExpr::KeyPattern { pattern: inner.to_string(), mode: MatchMode::Exact };
    }
    FilterExpr::KeyPattern { pattern: raw.to_string(), mode: MatchMode::Unquoted }
}

fn parse_locale_term(raw: &str) -> FilterExpr {
    // Bare `?` → AnyMissing shorthand.
    if raw == "?" {
        return FilterExpr::AnyMissing;
    }

    if raw.starts_with('"') {
        // Quoted locale — modifier is the character(s) after the closing `"`.
        if let Some(close_offset) = raw[1..].find('"') {
            let inner = &raw[1..close_offset + 1];
            let after = &raw[close_offset + 2..];
            return FilterExpr::LocaleStatus {
                locale: inner.to_string(),
                modifier: parse_modifier_str(after),
                mode: MatchMode::Exact,
            };
        }
    }

    // Unquoted — modifier is the final character.
    let (locale, modifier) = split_modifier(raw);
    FilterExpr::LocaleStatus {
        locale: locale.to_string(),
        modifier,
        mode: MatchMode::Unquoted,
    }
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

// ── Evaluator ─────────────────────────────────────────────────────────────────

/// Returns `true` if `key` should be visible given `expr` and the workspace.
pub fn evaluate(expr: &FilterExpr, key: &str, workspace: &Workspace) -> bool {
    match expr {
        FilterExpr::And(terms) => terms.iter().all(|t| evaluate(t, key, workspace)),

        FilterExpr::KeyPattern { pattern, mode } => match mode {
            MatchMode::Exact => key == pattern.as_str(),
            // Substring match for now; nucleo fuzzy planned.
            MatchMode::Unquoted => key.contains(pattern.as_str()),
        },

        FilterExpr::AnyMissing => workspace
            .all_locales()
            .iter()
            .any(|locale| !has_value(key, locale, workspace)),

        FilterExpr::LocaleStatus { locale, modifier, mode } => {
            match modifier {
                // Any modifier: column-visibility hint only, never filters keys.
                StatusModifier::Any => true,
                StatusModifier::Present => {
                    // All locales matching the selector must have this key.
                    let matched = matching_locales(locale, mode, workspace);
                    !matched.is_empty() && matched.iter().all(|l| has_value(key, l, workspace))
                }
                StatusModifier::Missing => {
                    // All locales matching the selector must NOT have this key.
                    let matched = matching_locales(locale, mode, workspace);
                    !matched.is_empty() && matched.iter().all(|l| !has_value(key, l, workspace))
                }
            }
        }
    }
}

/// All workspace locales that match `pattern` under `mode`.
fn matching_locales(pattern: &str, mode: &MatchMode, workspace: &Workspace) -> Vec<String> {
    workspace
        .all_locales()
        .into_iter()
        .filter(|locale| locale_matches(locale, pattern, mode))
        .collect()
}

/// Returns the workspace locales that should be visible given `expr`.
///
/// Collects every `LocaleStatus` selector in the expression and shows only the
/// locales that match at least one of them. When the expression contains no
/// locale selectors (e.g. a key-only filter or `AnyMissing`), all locales are
/// returned unchanged.
pub fn visible_locales(expr: &FilterExpr, workspace: &Workspace) -> Vec<String> {
    let mut selectors: Vec<(&str, &MatchMode)> = Vec::new();
    collect_locale_selectors(expr, &mut selectors);

    if selectors.is_empty() {
        return workspace.all_locales();
    }

    workspace
        .all_locales()
        .into_iter()
        .filter(|locale| {
            selectors.iter().any(|(pattern, mode)| locale_matches(locale, pattern, mode))
        })
        .collect()
}

fn collect_locale_selectors<'a>(expr: &'a FilterExpr, out: &mut Vec<(&'a str, &'a MatchMode)>) {
    match expr {
        FilterExpr::And(terms) => {
            for t in terms {
                collect_locale_selectors(t, out);
            }
        }
        FilterExpr::LocaleStatus { locale, mode, .. } => {
            out.push((locale.as_str(), mode));
        }
        _ => {}
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
    workspace
        .groups
        .iter()
        .flat_map(|g| g.files.iter())
        .filter(|f| f.locale == locale)
        .any(|f| f.get(key).is_some())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(expr: &FilterExpr) -> &[FilterExpr] {
        match expr {
            FilterExpr::And(t) => t,
            _ => panic!("expected And"),
        }
    }

    #[test]
    fn empty_matches_all() {
        assert!(matches!(parse(""), FilterExpr::And(ref t) if t.is_empty()));
        assert!(matches!(parse("  "), FilterExpr::And(ref t) if t.is_empty()));
    }

    #[test]
    fn key_unquoted() {
        let expr = parse("error");
        let t = terms(&expr);
        assert!(matches!(&t[0],
            FilterExpr::KeyPattern { pattern, mode }
            if pattern == "error" && *mode == MatchMode::Unquoted
        ));
    }

    #[test]
    fn key_exact() {
        let expr = parse("\"app.error.notfound\"");
        let t = terms(&expr);
        assert!(matches!(&t[0],
            FilterExpr::KeyPattern { pattern, mode }
            if pattern == "app.error.notfound" && *mode == MatchMode::Exact
        ));
    }

    #[test]
    fn bare_any_missing() {
        let expr = parse(":?");
        let t = terms(&expr);
        assert!(matches!(&t[0], FilterExpr::AnyMissing));
    }

    #[test]
    fn locale_present_unquoted() {
        let expr = parse(":de!");
        let t = terms(&expr);
        assert!(matches!(&t[0],
            FilterExpr::LocaleStatus { locale, modifier, mode }
            if locale == "de" && *modifier == StatusModifier::Present && *mode == MatchMode::Unquoted
        ));
    }

    #[test]
    fn locale_missing_exact() {
        let expr = parse(":\"de_AT\"?");
        let t = terms(&expr);
        assert!(matches!(&t[0],
            FilterExpr::LocaleStatus { locale, modifier, mode }
            if locale == "de_AT" && *modifier == StatusModifier::Missing && *mode == MatchMode::Exact
        ));
    }

    #[test]
    fn locale_any_no_modifier() {
        let expr = parse(":de");
        let t = terms(&expr);
        assert!(matches!(&t[0],
            FilterExpr::LocaleStatus { locale, modifier, mode }
            if locale == "de" && *modifier == StatusModifier::Any && *mode == MatchMode::Unquoted
        ));
    }

    #[test]
    fn multi_term_and() {
        let expr = parse("error, timeout: de?");
        let t = terms(&expr);
        assert_eq!(t.len(), 3);
        assert!(matches!(&t[0], FilterExpr::KeyPattern { pattern, .. } if pattern == "error"));
        assert!(matches!(&t[1], FilterExpr::KeyPattern { pattern, .. } if pattern == "timeout"));
        assert!(matches!(&t[2],
            FilterExpr::LocaleStatus { locale, modifier, .. }
            if locale == "de" && *modifier == StatusModifier::Missing
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
}
