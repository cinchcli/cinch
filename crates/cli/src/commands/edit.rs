//! `cinch edit [REF]` — open a clip's text in `$EDITOR`, save the result as a
//! NEW local clip, and copy it to the clipboard. REF is a clip id-prefix or a
//! relative index (1 = latest); omitted ⇒ latest. Editing never mutates the
//! original (see `client_core::edit::apply_edit`).

use std::io::IsTerminal;

use client_core::store::Store;

use crate::exit::{ExitError, GENERIC_ERROR};
use crate::io::{copy_text_to_clipboard, pick_editor, spawn_editor};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Clip id prefix (>=4 chars) or relative index (1 = latest). Omitted = latest.
    pub clip: Option<String>,
    /// Do not copy the edited result to the system clipboard.
    #[arg(long)]
    pub no_copy: bool,
}

/// Resolve the target clip's id and current UTF-8 text. Errors on image /
/// non-UTF-8 content (mirrors `transform.rs`).
pub(crate) fn resolve_clip_text(
    store: &Store,
    reference: Option<&str>,
) -> Result<(String, String), ExitError> {
    let clip = match reference {
        Some(r) if r.parse::<i64>().is_ok() => {
            let index: i64 = r.parse().unwrap();
            if index < 1 {
                return Err(ExitError::new(
                    GENERIC_ERROR,
                    "index must be at least 1 (1 = latest)",
                    "",
                ));
            }
            let mut rows = client_core::store::queries::list_clips(
                store,
                None,
                None,
                Some(1),
                Some(index - 1),
                None,
                false,
                1,
            )
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
            rows.pop().ok_or_else(|| {
                ExitError::new(GENERIC_ERROR, format!("no clip found at index {index}"), "")
            })?
        }
        Some(r) => {
            let id = client_core::store::prefix::resolve_clip_id(store, r)
                .map_err(crate::commands::get::render_resolve_error)?;
            client_core::store::queries::get_clip(store, &id)
                .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?
                .ok_or_else(|| {
                    ExitError::new(GENERIC_ERROR, "clip vanished after resolution", "")
                })?
        }
        None => {
            let mut rows = client_core::store::queries::list_clips(
                store,
                None,
                None,
                Some(1),
                Some(0),
                None,
                false,
                1,
            )
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
            rows.pop().ok_or_else(|| {
                ExitError::new(
                    GENERIC_ERROR,
                    "no clips in local history",
                    "Run: cinch copy",
                )
            })?
        }
    };

    let bytes = clip
        .content
        .ok_or_else(|| ExitError::new(GENERIC_ERROR, "clip has no text content", ""))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| ExitError::new(GENERIC_ERROR, "clip content is not valid UTF-8", ""))?;
    Ok((clip.id, text))
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    if !std::io::stdin().is_terminal() {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "cinch edit opens an interactive editor",
            "Run it from a terminal.",
        ));
    }

    let ctx = crate::runtime::open_ctx().map_err(|_| {
        ExitError::new(
            crate::exit::AUTH_FAILURE,
            "No auth token configured.",
            "Run: cinch auth login",
        )
    })?;
    crate::runtime::opportunistic_backfill(&ctx).await;

    let (clip_id, original_text) = resolve_clip_text(&ctx.store, args.clip.as_deref())?;

    let tmp = std::env::temp_dir().join(format!("cinch-edit-{}.txt", ulid::Ulid::new()));
    std::fs::write(&tmp, original_text.as_bytes())
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("temp write failed: {e}"), ""))?;

    let editor = pick_editor();
    let status = spawn_editor(&editor, &tmp);
    let edited = std::fs::read_to_string(&tmp).unwrap_or_default();
    let _ = std::fs::remove_file(&tmp);

    match status {
        Ok(s) if !s.success() => {
            return Err(ExitError::new(
                GENERIC_ERROR,
                "editor exited with an error",
                "",
            ));
        }
        Err(e) => {
            return Err(ExitError::new(
                GENERIC_ERROR,
                format!("could not launch editor '{editor}': {e}"),
                "",
            ));
        }
        Ok(_) => {}
    }

    if edited == original_text || edited.trim().is_empty() {
        eprintln!("No changes; clip not modified.");
        return Ok(());
    }

    let new_id = client_core::edit::apply_edit(&ctx.store, &clip_id, &edited)
        .map_err(|e| ExitError::new(GENERIC_ERROR, e.to_string(), ""))?;

    if !args.no_copy {
        copy_text_to_clipboard(&edited);
    }
    eprintln!("\u{2713} Saved edited clip to local history (id={new_id}).");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_core::store::models::{StoredClip, SyncState};
    use client_core::store::{queries, Store};
    use std::path::Path;

    fn store_with(id: &str, content: &[u8], ct: &str) -> Store {
        let store = Store::open(Path::new(":memory:")).unwrap();
        queries::insert_clip(
            &store,
            &StoredClip {
                id: id.to_string(),
                source: "local".to_string(),
                content_type: ct.to_string(),
                content: Some(content.to_vec()),
                byte_size: content.len() as i64,
                created_at: 1,
                sync_state: SyncState::Local,
                ..Default::default()
            },
        )
        .unwrap();
        store
    }

    #[test]
    fn resolve_clip_text_by_prefix_returns_text() {
        let store = store_with("01HXCCCCCCCCCCCCCCCCCCCCCC", b"![[a.webp|703]]", "text");
        let (id, text) = resolve_clip_text(&store, Some("01HX")).unwrap();
        assert_eq!(id, "01HXCCCCCCCCCCCCCCCCCCCCCC");
        assert_eq!(text, "![[a.webp|703]]");
    }

    #[test]
    fn resolve_clip_text_rejects_non_utf8() {
        // Non-UTF-8 content stored in the DB. We resolve via `None` (latest)
        // so we go through the `list_clips` path (which returns content as
        // Vec<u8>) rather than the prefix-lookup path (which fetches a preview
        // column as String and would fail before reaching the UTF-8 check).
        let store = store_with("01HXDDDDDDDDDDDDDDDDDDDDDD", &[0xff, 0xfe], "text");
        let err = resolve_clip_text(&store, None).unwrap_err();
        assert!(err.to_string().contains("not valid UTF-8"));
    }

    #[test]
    fn resolve_clip_text_latest_when_no_ref() {
        let store = store_with("01HXEEEEEEEEEEEEEEEEEEEEEE", b"only", "text");
        let (id, text) = resolve_clip_text(&store, None).unwrap();
        assert_eq!(id, "01HXEEEEEEEEEEEEEEEEEEEEEE");
        assert_eq!(text, "only");
    }
}
