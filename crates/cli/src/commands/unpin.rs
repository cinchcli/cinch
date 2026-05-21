//! `cinch unpin <id-prefix>` — unpin a clip locally and on the relay.

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// ID prefix (minimum 4 characters).
    pub id: String,
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

    let id = client_core::store::prefix::resolve_clip_id(&ctx.store, &args.id)
        .map_err(crate::commands::get::render_resolve_error)?;
    ctx.client
        .set_clip_pin(&id, false, None)
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("relay: {e}"), ""))?;
    // when_ms is ignored by the query when pinned=false (column is set to NULL).
    client_core::store::queries::set_pinned(&ctx.store, &id, false, 0)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
    println!("unpinned {id}");
    Ok(())
}
