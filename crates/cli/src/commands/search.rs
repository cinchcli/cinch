//! `cinch clip search` — FTS5 full-text search of the local clip store.

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Search query (FTS5 syntax: words, "phrase", AND, OR, NEAR/N).
    pub query: String,
    /// Max rows to return.
    #[arg(long, default_value_t = 20)]
    pub limit: i64,
    /// `text` (default) or `json`.
    #[arg(long, default_value = "text")]
    pub format: String,
    /// Filter by content type (e.g. text, image, url, code).
    #[arg(long = "type")]
    pub filter_type: Option<String>,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    let ctx = crate::runtime::open_ctx().map_err(|_| {
        ExitError::new(
            AUTH_FAILURE,
            "No auth token configured.",
            "Run: cinch auth login",
        )
    })?;
    crate::runtime::opportunistic_backfill(&ctx).await;

    let hits = client_core::store::queries::search_clips(
        &ctx.store,
        &args.query,
        args.limit,
        args.filter_type.as_deref(),
    )
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("search failed: {e}"), ""))?;

    if args.format == "json" {
        println!(
            "{}",
            serde_json::to_string(&hits).unwrap_or_else(|_| "[]".into())
        );
    } else {
        for c in &hits {
            let preview = if let Some(l) = &c.label {
                format!("[{l}]")
            } else {
                preview_of(c)
            };
            let when = crate::commands::list::format_unix_ms_as_rfc3339(c.created_at);
            let source_display = if let Some(app) = &c.source_app {
                format!("{} ({})", c.source, app)
            } else {
                c.source.clone()
            };
            println!(
                "{}  {:<20}  {}  {}",
                &c.id[..12],
                source_display,
                when,
                preview
            );
        }
    }
    Ok(())
}

fn preview_of(c: &client_core::store::models::StoredClip) -> String {
    if is_text_like(&c.content_type) {
        let raw = String::from_utf8_lossy(c.content.as_deref().unwrap_or(b""));
        let oneline = raw.replace('\n', " ");
        // Truncate by characters, not bytes, so multibyte content (Korean,
        // emoji, …) is never split mid-codepoint into U+FFFD replacements.
        let mut chars = oneline.chars();
        let head: String = chars.by_ref().take(40).collect();
        if chars.next().is_some() {
            format!("{head}…")
        } else {
            head
        }
    } else {
        format!("[{} · {}]", c.content_type, fmt_bytes(c.byte_size))
    }
}

/// Content types whose stored bytes are human-readable and should be previewed
/// inline. Covers the canonical vocabulary (`text`, `code`, `url`) plus legacy
/// MIME values (`text/plain`, …) from pre-canonicalization clips. Mirrors
/// `compact_type` in `list.rs` and `isTextLike` in the desktop frontend.
fn is_text_like(ct: &str) -> bool {
    ct.starts_with("text") || ct == "code" || ct == "url"
}

fn fmt_bytes(n: i64) -> String {
    if n < 1024 {
        format!("{n}B")
    } else if n < 1024 * 1024 {
        format!("{:.0}KB", n as f64 / 1024.0)
    } else {
        format!("{:.1}MB", n as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(
        content_type: &str,
        content: Option<Vec<u8>>,
        byte_size: i64,
    ) -> client_core::store::models::StoredClip {
        client_core::store::models::StoredClip {
            id: "01HXXXXXXXXXXXXXXXXXXXXXXX".into(),
            source: "test".into(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: content_type.into(),
            content,
            media_path: None,
            byte_size,
            created_at: 0,
            pinned: false,
            pinned_at: None,
            sync_state: client_core::store::models::SyncState::Synced,
        }
    }

    #[test]
    fn test_preview_of_text_short() {
        let c = clip("text/plain", Some(b"hello world".to_vec()), 11);
        assert_eq!(preview_of(&c), "hello world");
    }

    #[test]
    fn test_preview_of_canonical_text_shows_content() {
        // Canonical content_type is the bare string "text" (no MIME slash).
        // The preview must show the matched content, not a "[text · NB]" placeholder.
        let c = clip("text", Some(b"@MuClaw hi".to_vec()), 10);
        assert_eq!(preview_of(&c), "@MuClaw hi");
    }

    #[test]
    fn test_preview_of_canonical_code_and_url_show_content() {
        let code = clip("code", Some(b"fn main() {}".to_vec()), 12);
        assert_eq!(preview_of(&code), "fn main() {}");
        let url = clip("url", Some(b"https://example.com".to_vec()), 19);
        assert_eq!(preview_of(&url), "https://example.com");
    }

    #[test]
    fn test_preview_of_multibyte_truncates_by_char_not_byte() {
        // 50 Korean chars (3 bytes each = 150 bytes). Byte-based truncation would
        // split a multibyte boundary and emit replacement chars; char-based must
        // take the first 40 chars cleanly and append an ellipsis.
        let content = "가".repeat(50);
        let c = clip("text", Some(content.into_bytes()), 150);
        let preview = preview_of(&c);
        assert!(
            !preview.contains('\u{FFFD}'),
            "preview must not contain U+FFFD"
        );
        assert_eq!(preview, format!("{}…", "가".repeat(40)));
    }

    #[test]
    fn test_preview_of_canonical_text_no_ellipsis_when_short() {
        // 30 Korean chars (< 40): whole string shown, no ellipsis.
        let content = "가".repeat(30);
        let c = clip("text", Some(content.clone().into_bytes()), 90);
        assert_eq!(preview_of(&c), content);
    }

    #[test]
    fn test_preview_of_text_truncates_at_40() {
        let c = clip("text/plain", Some(vec![b'a'; 80]), 80);
        let expected = format!("{}…", "a".repeat(40));
        assert_eq!(preview_of(&c), expected);
    }

    #[test]
    fn test_preview_of_binary() {
        let c = clip("image/png", None, 2048);
        assert_eq!(preview_of(&c), "[image/png · 2KB]");
    }

    #[test]
    fn test_fmt_bytes() {
        assert_eq!(fmt_bytes(512), "512B");
        assert_eq!(fmt_bytes(2048), "2KB");
        assert_eq!(fmt_bytes(1_572_864), "1.5MB");
    }
}
