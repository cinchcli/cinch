//! `cinch sources` — distinct source machines that have ever pushed clips.

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// One name per line, no header.
    #[arg(long)]
    pub names: bool,
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

    let rows = client_core::store::queries::list_sources(&ctx.store)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;

    if args.names {
        for r in &rows {
            println!("{}", r.source);
        }
        return Ok(());
    }

    if args.format == "json" {
        let s = serde_json::to_string(&rows)
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("serialize: {e}"), ""))?;
        println!("{s}");
        return Ok(());
    }

    println!("  {:<24}  {:>6}  {}", "SOURCE", "CLIPS", "LAST SEEN");
    for r in &rows {
        let last_seen = crate::fmt::fmt_last_seen(r.last_seen);
        println!("  {:<24}  {:>6}  {}", r.source, r.clip_count, last_seen);
    }
    Ok(())
}
