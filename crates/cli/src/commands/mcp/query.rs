//! Pure, defensive helpers for turning untrusted MCP input into safe queries.

pub const DEFAULT_LIMIT: i64 = 20;
pub const MAX_LIMIT: i64 = 100;

/// Make an arbitrary natural-language query safe for SQLite FTS5 `MATCH`.
/// FTS5 treats `-`, `:`, `"`, `*`, `(`, `)`, `^`, and bare-word operators
/// (`AND`/`OR`/`NEAR`) specially; raw AI/user input often produces syntax
/// errors. We split on whitespace and wrap each token as a quoted FTS5 string
/// (internal `"` doubled), joined with spaces (implicit AND). Whitespace-only
/// input yields `""`, which callers treat as "no FTS filter".
pub fn sanitize_fts_query(raw: &str) -> String {
    raw.split_whitespace()
        .map(|tok| format!("\"{}\"", tok.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Clamp a client-supplied limit into a safe range. `None`, zero, or negative
/// fall back to the default; anything above the hard maximum is capped.
pub fn clamp_limit(requested: Option<i64>) -> i64 {
    match requested {
        Some(n) if n >= 1 => n.min(MAX_LIMIT),
        _ => DEFAULT_LIMIT,
    }
}

/// Parse the exposure-scope env value (`CINCH_MCP_MAX_AGE_DAYS`) into a max age
/// in days. Returns `None` (unbounded — expose full history) for unset, empty,
/// non-positive, or invalid input. This is the opt-in privacy lever: the
/// surface is full history by default and only narrows when the user sets it.
pub fn parse_max_age_days(raw: Option<&str>) -> Option<i64> {
    raw.and_then(|s| s.trim().parse::<i64>().ok())
        .filter(|&n| n > 0)
}

/// Compute the `since_ms` cutoff from a max-age window relative to `now_ms`.
/// `None` window means no cutoff. Returns `None` if the multiplication would overflow.
pub fn since_ms_from_days(now_ms: i64, max_age_days: Option<i64>) -> Option<i64> {
    max_age_days
        .and_then(|days| days.checked_mul(86_400_000))
        .map(|window| now_ms - window)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_quotes_each_token() {
        assert_eq!(sanitize_fts_query("hello world"), "\"hello\" \"world\"");
    }

    #[test]
    fn sanitize_neutralizes_fts_operators() {
        // `-`, `:`, `*` would be FTS5 operators if unquoted; one token stays one quoted token.
        assert_eq!(sanitize_fts_query("foo-bar:baz*"), "\"foo-bar:baz*\"");
    }

    #[test]
    fn sanitize_doubles_embedded_quotes() {
        assert_eq!(sanitize_fts_query("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn sanitize_empty_is_empty() {
        assert_eq!(sanitize_fts_query("   "), "");
    }

    #[test]
    fn clamp_defaults_and_caps() {
        assert_eq!(clamp_limit(None), DEFAULT_LIMIT);
        assert_eq!(clamp_limit(Some(0)), DEFAULT_LIMIT);
        assert_eq!(clamp_limit(Some(-5)), DEFAULT_LIMIT);
        assert_eq!(clamp_limit(Some(5)), 5);
        assert_eq!(clamp_limit(Some(9999)), MAX_LIMIT);
    }

    #[test]
    fn max_age_parses_only_positive_ints() {
        assert_eq!(parse_max_age_days(None), None);
        assert_eq!(parse_max_age_days(Some("")), None);
        assert_eq!(parse_max_age_days(Some("0")), None);
        assert_eq!(parse_max_age_days(Some("-3")), None);
        assert_eq!(parse_max_age_days(Some("nope")), None);
        assert_eq!(parse_max_age_days(Some(" 90 ")), Some(90));
    }

    #[test]
    fn since_cutoff_subtracts_window() {
        let now = 1_700_000_000_000;
        assert_eq!(since_ms_from_days(now, None), None);
        assert_eq!(since_ms_from_days(now, Some(1)), Some(now - 86_400_000));
    }

    #[test]
    fn since_cutoff_does_not_overflow() {
        assert_eq!(since_ms_from_days(0, Some(i64::MAX)), None);
    }
}
