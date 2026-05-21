//! Shared formatters for CLI output.

/// Formats an optional unix-millisecond timestamp as an RFC-3339 / ISO-8601
/// UTC string, returning `"—"` for `None`. Delegates to
/// `crate::commands::list::format_unix_ms_as_rfc3339` for the `Some` case.
pub(crate) fn fmt_last_seen(ts: Option<i64>) -> String {
    ts.map(crate::commands::list::format_unix_ms_as_rfc3339)
        .unwrap_or_else(|| "—".into())
}
