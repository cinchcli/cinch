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

    let hits = client_core::store::queries::search_clips(&ctx.store, &args.query, args.limit)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("search failed: {e}"), ""))?;

    if args.format == "json" {
        println!(
            "{}",
            serde_json::to_string(&hits).unwrap_or_else(|_| "[]".into())
        );
    } else {
        for c in &hits {
            let preview = preview_of(c);
            let when = crate::commands::list::format_unix_ms_as_rfc3339(c.created_at);
            println!("{}  {:<14}  {}  {}", &c.id[..12], c.source, when, preview);
        }
    }
    Ok(())
}

fn preview_of(c: &client_core::store::models::StoredClip) -> String {
    if c.content_type.starts_with("text/") {
        let text: Vec<u8> = c
            .content
            .as_deref()
            .unwrap_or(b"")
            .iter()
            .take(40)
            .copied()
            .collect();
        let s = String::from_utf8_lossy(&text).replace('\n', " ");
        if s.len() == 40 {
            format!("{s}…")
        } else {
            s
        }
    } else {
        format!("[{} · {}]", c.content_type, fmt_bytes(c.byte_size))
    }
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
