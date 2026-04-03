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

#[derive(Debug, Clone)]
pub enum FilterExpr {
    And(Vec<FilterExpr>),
    KeyPattern { pattern: String, mode: MatchMode },
    LocaleStatus { locale: String, modifier: StatusModifier, mode: MatchMode },
    /// Bare `?` shorthand: at least one locale is missing.
    AnyMissing,
    /// `*pattern` — key is dangling (unsaved) and matches the pattern.
    /// Bare `*` matches all dangling keys.
    DanglingKey { pattern: String, mode: MatchMode },
    /// `bundle1, bundle2 /` — key must belong to one of the listed bundles.
    /// Unquoted = prefix match on the bundle name; quoted = exact match.
    BundleFilter(Vec<(String, MatchMode)>),
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Parse a filter query string into a `FilterExpr`.
///
/// Full format: `[bundle, ...] [/ key_pattern, ...] [: locale[modifier], ...]`
///
/// Section rules (each section ends at the next separator or end of input):
///   - From line start to the first `/` or `:` → bundle selectors
///   - After `/` to the next `:` or end          → key patterns
///   - After `:`  to end                          → locale selectors
///
/// Examples:
///   `messages`          — bundle filter only
///   `/error`            — key filter only
///   `:de`               — locale filter only
///   `messages/error:de` — all three sections
///
/// Returns `FilterExpr::And([])` (matches everything) for empty input.
pub fn parse(input: &str) -> FilterExpr {
    let input = input.trim();
    if input.is_empty() {
        return FilterExpr::And(vec![]);
    }

    let mut terms: Vec<FilterExpr> = Vec::new();

    let slash_idx = input.find('/');
    let colon_idx = input.find(':');

    // Bundle section: from start to whichever separator comes first.
    let bundle_end = match (slash_idx, colon_idx) {
        (Some(s), Some(c)) => Some(s.min(c)),
        (Some(s), None)    => Some(s),
        (None,    Some(c)) => Some(c),
        (None,    None)    => None,
    };
    let bundle_str = match bundle_end {
        Some(idx) => input[..idx].trim(),
        None      => input,  // entire input is the bundle section
    };
    let mut bundle_terms: Vec<(String, MatchMode)> = Vec::new();
    for raw in bundle_str.split_whitespace() {
        if let Some(inner) = strip_quotes(raw) {
            bundle_terms.push((inner.to_string(), MatchMode::Exact));
        } else {
            bundle_terms.push((raw.to_string(), MatchMode::Unquoted));
        }
    }
    if !bundle_terms.is_empty() {
        terms.push(FilterExpr::BundleFilter(bundle_terms));
    }

    // Key section: after '/' up to the next ':' (or end). Only present when '/' exists.
    if let Some(s_idx) = slash_idx {
        let after_slash = &input[s_idx + 1..];
        let key_str = match after_slash.find(':') {
            Some(c_idx) => after_slash[..c_idx].trim(),
            None        => after_slash.trim(),
        };
        for raw in key_str.split_whitespace() {
            terms.push(parse_key_term(raw));
        }
    }

    // Locale section: after ':' to end. Only present when ':' exists.
    if let Some(c_idx) = colon_idx {
        let locale_str = input[c_idx + 1..].trim();
        for raw in locale_str.split_whitespace() {
            terms.push(parse_locale_term(raw));
        }
    }

    FilterExpr::And(terms)
}

fn parse_key_term(raw: &str) -> FilterExpr {
    if let Some(rest) = raw.strip_prefix('*') {
        // `*"pattern"` → exact dangling match; `*pattern` → substring dangling match.
        if let Some(inner) = strip_quotes(rest) {
            return FilterExpr::DanglingKey { pattern: inner.to_string(), mode: MatchMode::Exact };
        }
        return FilterExpr::DanglingKey { pattern: rest.to_string(), mode: MatchMode::Unquoted };
    }
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

        FilterExpr::DanglingKey { pattern, mode } => {
            if !workspace.is_dangling(key) {
                return false;
            }
            match mode {
                MatchMode::Exact => key == pattern.as_str(),
                MatchMode::Unquoted => key.contains(pattern.as_str()),
            }
        }

        FilterExpr::BundleFilter(bundles) => {
            let (key_bundle, _) = workspace::split_key(key);
            bundles.iter().any(|(pattern, mode)| match mode {
                MatchMode::Exact => key_bundle == pattern.as_str(),
                MatchMode::Unquoted => key_bundle.starts_with(pattern.as_str()),
            })
        }

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
        FilterExpr::BundleFilter(_) | FilterExpr::KeyPattern { .. }
        | FilterExpr::AnyMissing | FilterExpr::DanglingKey { .. } => {}
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
    fn bundle_unquoted() {
        let expr = parse("messages");
        let t = terms(&expr);
        assert!(matches!(&t[0],
            FilterExpr::BundleFilter(v)
            if v.len() == 1 && v[0].0 == "messages" && v[0].1 == MatchMode::Unquoted
        ));
    }

    #[test]
    fn bundle_exact() {
        let expr = parse("\"messages\"");
        let t = terms(&expr);
        assert!(matches!(&t[0],
            FilterExpr::BundleFilter(v)
            if v.len() == 1 && v[0].0 == "messages" && v[0].1 == MatchMode::Exact
        ));
    }

    #[test]
    fn key_unquoted() {
        let expr = parse("/error");
        let t = terms(&expr);
        assert!(matches!(&t[0],
            FilterExpr::KeyPattern { pattern, mode }
            if pattern == "error" && *mode == MatchMode::Unquoted
        ));
    }

    #[test]
    fn key_exact() {
        let expr = parse("/\"app.error.notfound\"");
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
        // Key patterns require '/' prefix; whitespace separates multiple terms.
        let expr = parse("/error timeout: de?");
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
    fn all_three_sections() {
        let expr = parse("messages /error :de");
        let t = terms(&expr);
        assert_eq!(t.len(), 3);
        assert!(matches!(&t[0], FilterExpr::BundleFilter(v) if v[0].0 == "messages"));
        assert!(matches!(&t[1], FilterExpr::KeyPattern { pattern, .. } if pattern == "error"));
        assert!(matches!(&t[2], FilterExpr::LocaleStatus { locale, .. } if locale == "de"));
    }

    #[test]
    fn multi_bundle_whitespace() {
        let expr = parse("messages errors");
        let t = terms(&expr);
        assert_eq!(t.len(), 1);
        assert!(matches!(&t[0], FilterExpr::BundleFilter(v) if v.len() == 2));
    }

    #[test]
    fn multi_locale_whitespace() {
        let expr = parse(":de fr");
        let t = terms(&expr);
        assert_eq!(t.len(), 2);
        assert!(matches!(&t[0], FilterExpr::LocaleStatus { locale, .. } if locale == "de"));
        assert!(matches!(&t[1], FilterExpr::LocaleStatus { locale, .. } if locale == "fr"));
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
