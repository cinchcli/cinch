use std::io::Write;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Clip id or unique id prefix.
    pub clip: Option<String>,
    /// Transform action id, e.g. pretty-json or redact-secrets.
    #[arg(long, short = 'a')]
    pub action: Option<String>,
    /// List available transform actions.
    #[arg(long)]
    pub list_actions: bool,
    /// Copy transformed output to the system clipboard instead of stdout.
    #[arg(long)]
    pub copy: bool,
}

pub(crate) fn transform_clip_from_store(
    store: &client_core::store::Store,
    clip_prefix: &str,
    action_id: &str,
) -> Result<String, crate::exit::ExitError> {
    let id = client_core::store::prefix::resolve_clip_id(store, clip_prefix)
        .map_err(crate::commands::get::render_resolve_error)?;
    let clip = client_core::store::queries::get_clip(store, &id)
        .map_err(|e| crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, e.to_string(), ""))?
        .ok_or_else(|| {
            crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, "clip not found", "")
        })?;
    let content = clip.content.as_deref().ok_or_else(|| {
        crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, "clip has no text content", "")
    })?;
    let text = std::str::from_utf8(content).map_err(|_| {
        crate::exit::ExitError::new(
            crate::exit::GENERIC_ERROR,
            "clip content is not valid UTF-8",
            "",
        )
    })?;
    let action = client_core::transform::TransformAction::from_id(action_id).ok_or_else(|| {
        crate::exit::ExitError::new(
            crate::exit::GENERIC_ERROR,
            format!("unknown transform action: {action_id}"),
            "",
        )
    })?;
    client_core::transform::apply_transform(action, text, &clip.content_type)
        .map_err(|e| crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, e.to_string(), ""))
}

pub async fn run(args: Args) -> Result<(), crate::exit::ExitError> {
    if args.list_actions {
        for a in client_core::transform::list_transform_actions("text") {
            println!("{}\t{}", a.id, a.label);
        }
        return Ok(());
    }

    let clip = args.clip.as_deref().ok_or_else(|| {
        crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, "missing clip id", "")
    })?;
    let action = args.action.as_deref().ok_or_else(|| {
        crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, "missing --action", "")
    })?;

    let ctx = crate::runtime::open_ctx().map_err(|_| {
        crate::exit::ExitError::new(
            crate::exit::AUTH_FAILURE,
            "No auth token configured.",
            "Run: cinch auth login",
        )
    })?;
    crate::runtime::opportunistic_backfill(&ctx).await;

    let out = transform_clip_from_store(&ctx.store, clip, action)?;
    if args.copy {
        copy_text_to_clipboard(&out)?;
    } else {
        write_to_stdout(out.as_bytes())?;
    }
    Ok(())
}

fn write_to_stdout(bytes: &[u8]) -> Result<(), crate::exit::ExitError> {
    match std::io::stdout().write_all(bytes) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(crate::exit::ExitError::new(
            crate::exit::GENERIC_ERROR,
            e.to_string(),
            "",
        )),
    }
}

fn copy_text_to_clipboard(text: &str) -> Result<(), crate::exit::ExitError> {
    let mut cb = arboard::Clipboard::new().map_err(|e| {
        crate::exit::ExitError::new(
            crate::exit::GENERIC_ERROR,
            format!("could not open clipboard: {e}"),
            "",
        )
    })?;
    cb.set_text(text).map_err(|e| {
        crate::exit::ExitError::new(
            crate::exit::GENERIC_ERROR,
            format!("clipboard write failed: {e}"),
            "",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_core::store::{
        models::{StoredClip, SyncState},
        queries, Store,
    };
    use std::path::Path;

    fn store_with_clip(id: &str, content: &[u8], content_type: &str) -> Store {
        let store = Store::open(Path::new(":memory:")).unwrap();
        queries::insert_clip(
            &store,
            &StoredClip {
                id: id.to_string(),
                source: "local".to_string(),
                source_key: None,
                content_type: content_type.to_string(),
                content: Some(content.to_vec()),
                media_path: None,
                byte_size: content.len() as i64,
                created_at: 1,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Local,
            },
        )
        .unwrap();
        store
    }

    #[test]
    fn transform_clip_by_prefix_returns_text() {
        let store = store_with_clip("01HXABCDEFGHABCDEFGHABCD", br#"{"a":1}"#, "json");
        let out = transform_clip_from_store(&store, "01HX", "pretty-json").unwrap();
        assert_eq!(out, "{\n  \"a\": 1\n}");
    }

    #[test]
    fn transform_clip_rejects_image() {
        let store = store_with_clip("01HXABCDEFGHABCDEFGHABCD", b"png", "image");
        let err = transform_clip_from_store(&store, "01HX", "trim-whitespace").unwrap_err();
        assert!(err.to_string().contains("unsupported content type"));
    }
}
