//! `cinch clip get <id-prefix>` — print a single clip's contents (or metadata).

use client_core::store::models::ResolveError;

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};
use crate::io::{copy_text_to_clipboard, write_to_stdout};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// ID prefix (minimum 4 characters) or relative index (1 = latest, 2 = second latest, ...).
    pub id_or_index: String,
    /// Print metadata only, not content.
    #[arg(long)]
    pub meta: bool,
    /// Copy text content to system clipboard (TTY only).
    #[arg(long)]
    pub copy: bool,
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

    // Determine if input is a relative index (1-based) or a clip ID prefix.
    let clip = if let Ok(index) = args.id_or_index.parse::<i64>() {
        if index < 1 {
            return Err(ExitError::new(
                GENERIC_ERROR,
                "index must be at least 1 (1 = latest)",
                "",
            ));
        }
        // Fetch the n-th latest clip using OFFSET.
        let mut rows = client_core::store::queries::list_clips(
            &ctx.store,
            None,
            Some(1),
            Some(index - 1),
            None,
            false,
            1,
        )
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;

        rows.pop().ok_or_else(|| {
            ExitError::new(
                GENERIC_ERROR,
                format!("no clip found at index {index}"),
                "Run 'cinch clip list' to see available clips",
            )
        })?
    } else {
        let id = client_core::store::prefix::resolve_clip_id(&ctx.store, &args.id_or_index)
            .map_err(render_resolve_error)?;
        client_core::store::queries::get_clip(&ctx.store, &id)
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?
            .ok_or_else(|| ExitError::new(GENERIC_ERROR, "clip vanished after resolution", ""))?
    };

    if args.meta {
        println!("id:           {}", clip.id);
        println!("source:       {}", clip.source);
        if let Some(app) = clip.source_app {
            println!("source_app:   {}", app);
        }
        if let Some(url) = clip.source_url {
            println!("source_url:   {}", url);
        }
        if let Some(label) = clip.label {
            println!("label:        {}", label);
        }
        println!("content_type: {}", clip.content_type);
        println!("byte_size:    {}", clip.byte_size);
        println!(
            "created_at:   {}",
            crate::commands::list::format_unix_ms_as_rfc3339(clip.created_at)
        );
        println!("pinned:       {}", clip.pinned);
        return Ok(());
    }

    let content_bytes = if let Some(bytes) = clip.content {
        bytes
    } else if let Some(path) = clip.media_path {
        let abs = client_core::store::default_media_root()
            .map_err(|e| ExitError::new(GENERIC_ERROR, e.to_string(), ""))?
            .join(path);
        std::fs::read(&abs)
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("read media: {e}"), ""))?
    } else {
        Vec::new()
    };

    if !content_bytes.is_empty() {
        write_to_stdout(&content_bytes)?;

        if args.copy && !clip.content_type.starts_with("image") {
            if let Ok(text) = String::from_utf8(content_bytes) {
                copy_text_to_clipboard(&text);
            }
        } else if args.copy && clip.content_type.starts_with("image") {
            #[cfg(target_os = "macos")]
            {
                if let Err(e) = crate::macos_pasteboard::write_png(&content_bytes) {
                    eprintln!("Warning: image clipboard write failed: {}", e);
                }
            }
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("Warning: --copy for images is only supported on macOS.");
            }
        }
    }

    Ok(())
}

pub fn render_resolve_error(err: ResolveError) -> ExitError {
    match err {
        ResolveError::TooShort => {
            ExitError::new(GENERIC_ERROR, "prefix must be at least 4 characters", "")
        }
        ResolveError::NotFound => ExitError::new(GENERIC_ERROR, "no clip with that prefix", ""),
        ResolveError::Ambiguous { candidates } => {
            let mut msg = String::from("ambiguous prefix — multiple matches:\n");
            for c in candidates {
                let when = crate::commands::list::format_unix_ms_as_rfc3339(c.created_at);
                let preview = if let Some(l) = c.label {
                    format!("[{l}]")
                } else {
                    c.preview
                };
                msg.push_str(&format!(
                    "  {}  {:<14}  {}  {}\n",
                    &c.id[..12],
                    c.source,
                    when,
                    preview
                ));
            }
            ExitError::new(GENERIC_ERROR, msg, "re-run with a longer prefix")
        }
        ResolveError::Store(e) => ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use client_core::store::models::MatchInfo;

    #[derive(Debug, Parser)]
    #[command(no_binary_name = true)]
    struct GetArgsHarness {
        #[command(flatten)]
        args: Args,
    }

    #[test]
    fn test_parse_index() {
        let harness = GetArgsHarness::try_parse_from(["1"]).expect("parse ok");
        assert_eq!(harness.args.id_or_index, "1");
        assert!(!harness.args.copy);
    }

    #[test]
    fn test_parse_id_prefix() {
        let harness = GetArgsHarness::try_parse_from(["01HXXXXX"]).expect("parse ok");
        assert_eq!(harness.args.id_or_index, "01HXXXXX");
    }

    #[test]
    fn test_parse_with_copy() {
        let harness = GetArgsHarness::try_parse_from(["1", "--copy"]).expect("parse ok");
        assert!(harness.args.copy);
    }

    fn mk_candidate(id: &str, source: &str) -> MatchInfo {
        MatchInfo {
            id: id.to_string(),
            source: source.to_string(),
            content_type: "text/plain".into(),
            label: None,
            created_at: 0,
            preview: "preview".into(),
        }
    }

    #[test]
    fn test_render_too_short() {
        let err = render_resolve_error(ResolveError::TooShort);
        assert!(
            err.message.contains("at least 4"),
            "message was: {}",
            err.message
        );
    }

    #[test]
    fn test_render_not_found() {
        let err = render_resolve_error(ResolveError::NotFound);
        assert!(
            err.message.contains("no clip"),
            "message was: {}",
            err.message
        );
    }

    #[test]
    fn test_render_ambiguous_lists_candidates() {
        let err = render_resolve_error(ResolveError::Ambiguous {
            candidates: vec![
                mk_candidate("01HXXXXXXXXXXXXXXXXXXXXXXX", "device-a"),
                mk_candidate("01HZZZZZZZZZZZZZZZZZZZZZZZ", "device-b"),
            ],
        });
        assert!(
            err.message.contains("01HXXXXXXXXX"),
            "message was: {}",
            err.message
        );
        assert!(
            err.message.contains("01HZZZZZZZZZ"),
            "message was: {}",
            err.message
        );
        assert!(err.fix.contains("longer prefix"), "fix was: {}", err.fix);
    }
}
