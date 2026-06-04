//! `cinch paste [REF]` — print a LOCAL clip to stdout (pbpaste-shaped).
//!
//! The local-read counterpart to `cinch copy`. Resolves a REF (latest |
//! id-prefix | index; default latest) against the LOCAL store and prints the
//! clip content to stdout, pipe-friendly. This command is local-only: it never
//! contacts the relay (no backfill, no network).

use client_core::store::models::ResolveError;
use client_core::store::{self, queries, Store};

use crate::exit::{ExitError, GENERIC_ERROR};
use crate::io::{copy_text_to_clipboard, write_to_stdout};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Clip reference: `latest`, an id prefix (min 4 chars), or a relative
    /// index (1 = latest, 2 = second latest, ...). Defaults to `latest`.
    pub reference: Option<String>,
    /// Print metadata only, not content.
    #[arg(long)]
    pub meta: bool,
    /// Also write text content to the system clipboard (TTY only).
    #[arg(long)]
    pub copy: bool,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    // Local-only: open the store directly, never build a RestClient and never
    // run a backfill. `paste` is the [L] read counterpart to `copy`.
    let store_path = store::default_db_path()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store path: {e}"), ""))?;
    let store = Store::open(&store_path)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("open store: {e}"), ""))?;

    // Resolve the reference: `latest` / default → index 1; an integer → index;
    // otherwise an id prefix.
    let reference = args.reference.as_deref().unwrap_or("latest");
    let clip = resolve(&store, reference)?;

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

/// Resolve a REF to a single stored clip. `latest` and integer indices use
/// `list_clips` with OFFSET (1 = latest); anything else is an id prefix.
fn resolve(
    store: &Store,
    reference: &str,
) -> Result<client_core::store::models::StoredClip, ExitError> {
    // `latest` is sugar for index 1.
    let index_opt: Option<i64> = if reference.eq_ignore_ascii_case("latest") {
        Some(1)
    } else {
        reference.parse::<i64>().ok()
    };

    if let Some(index) = index_opt {
        if index < 1 {
            return Err(ExitError::new(
                GENERIC_ERROR,
                "index must be at least 1 (1 = latest)",
                "",
            ));
        }
        let mut rows =
            queries::list_clips(store, None, None, Some(1), Some(index - 1), None, false, 1)
                .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
        rows.pop().ok_or_else(|| {
            ExitError::new(
                GENERIC_ERROR,
                format!("no clip found at index {index}"),
                "Run 'cinch history list' to see available clips",
            )
        })
    } else {
        let id = client_core::store::prefix::resolve_clip_id(store, reference)
            .map_err(render_resolve_error)?;
        queries::get_clip(store, &id)
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?
            .ok_or_else(|| ExitError::new(GENERIC_ERROR, "clip vanished after resolution", ""))
    }
}

fn render_resolve_error(err: ResolveError) -> ExitError {
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
    use client_core::store::models::{StoredClip, SyncState};

    fn seed(store: &Store, id: &str, content: &[u8], created_at: i64) {
        let stored = StoredClip {
            id: id.to_string(),
            source: "remote:test-host".to_string(),
            content_type: "text".to_string(),
            content: Some(content.to_vec()),
            byte_size: content.len() as i64,
            created_at,
            sync_state: SyncState::Local,
            ..Default::default()
        };
        queries::insert_clip(store, &stored).unwrap();
    }

    fn mem_store() -> Store {
        Store::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn resolve_latest_returns_newest() {
        let store = mem_store();
        seed(&store, "01HAAAAAAAAAAAAAAAAAAAAAAA", b"older", 1000);
        seed(&store, "01HBBBBBBBBBBBBBBBBBBBBBBB", b"newer", 2000);
        let clip = resolve(&store, "latest").expect("latest resolves");
        assert_eq!(clip.content.as_deref(), Some(&b"newer"[..]));
    }

    #[test]
    fn resolve_index_two_returns_second_newest() {
        let store = mem_store();
        seed(&store, "01HAAAAAAAAAAAAAAAAAAAAAAA", b"older", 1000);
        seed(&store, "01HBBBBBBBBBBBBBBBBBBBBBBB", b"newer", 2000);
        let clip = resolve(&store, "2").expect("index 2 resolves");
        assert_eq!(clip.content.as_deref(), Some(&b"older"[..]));
    }

    #[test]
    fn resolve_id_prefix_returns_match() {
        let store = mem_store();
        seed(&store, "01HAAAAAAAAAAAAAAAAAAAAAAA", b"alpha", 1000);
        seed(&store, "01HBBBBBBBBBBBBBBBBBBBBBBB", b"beta", 2000);
        let clip = resolve(&store, "01HAAAA").expect("prefix resolves");
        assert_eq!(clip.content.as_deref(), Some(&b"alpha"[..]));
    }

    #[test]
    fn resolve_index_zero_errors() {
        let store = mem_store();
        seed(&store, "01HAAAAAAAAAAAAAAAAAAAAAAA", b"x", 1000);
        let err = resolve(&store, "0").expect_err("index 0 is invalid");
        assert_eq!(err.code, GENERIC_ERROR);
    }

    #[test]
    fn resolve_missing_index_errors() {
        let store = mem_store();
        let err = resolve(&store, "1").expect_err("empty store has no latest");
        assert_eq!(err.code, GENERIC_ERROR);
    }

    #[test]
    fn meta_only_arg_parses_and_default_ref_is_latest() {
        // The default reference is `latest` when none is supplied.
        let args = Args {
            reference: None,
            meta: true,
            copy: false,
        };
        assert_eq!(args.reference.as_deref().unwrap_or("latest"), "latest");
    }
}
