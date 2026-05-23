//! `cinch device retention [--device <id|self>] [--days N]` — view or set
//! the per-device remote retention (in days). Writes are limited to the
//! currently-authenticated device (the relay REST endpoint is
//! `/devices/self/retention`).

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Device ID prefix or `self` for this device (read-only for non-self).
    #[arg(long)]
    pub device: Option<String>,
    /// New retention in days; omit to read the current value(s).
    #[arg(long)]
    pub days: Option<i64>,
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

    let self_id = client_core::auth::load_config()
        .ok()
        .map(|c| c.active_device_id)
        .filter(|s| !s.is_empty());

    // Resolve --device to a full ID (or None for the "no filter" / "all rows" view).
    let target: Option<String> = match args.device.as_deref() {
        None => None,
        Some("self") => Some(self_id.clone().ok_or_else(|| {
            ExitError::new(
                GENERIC_ERROR,
                "no active device in config",
                "Run: cinch auth login",
            )
        })?),
        Some(prefix) => Some(
            client_core::store::prefix::resolve_device_id(&ctx.store, prefix)
                .map_err(crate::commands::get::render_resolve_error)?,
        ),
    };

    match (target.as_deref(), args.days) {
        (Some(id), Some(days)) => {
            if self_id.as_deref() != Some(id) {
                return Err(ExitError::new(
                    GENERIC_ERROR,
                    "writing retention for another device is not supported over REST",
                    "Pair into that device and run: cinch device retention --device self --days N",
                ));
            }
            ctx.client
                .set_remote_retention(days as i32)
                .await
                .map_err(|e| ExitError::new(GENERIC_ERROR, format!("relay: {e}"), ""))?;
            client_core::store::queries::set_retention(&ctx.store, id, days)
                .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
            println!("{id}: retention set to {days} days");
        }
        (Some(id), None) => {
            let prefs = client_core::store::queries::list_retention(&ctx.store)
                .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
            match prefs.into_iter().find(|p| p.device_id == id) {
                Some(p) => println!("{id}: {} days", p.days),
                None => println!("{id}: (default — no override)"),
            }
        }
        (None, Some(_)) => {
            return Err(ExitError::new(
                GENERIC_ERROR,
                "--days requires --device (use --device self for this device)",
                "",
            ));
        }
        (None, None) => {
            let prefs = client_core::store::queries::list_retention(&ctx.store)
                .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
            if prefs.is_empty() {
                println!("(no per-device overrides)");
                return Ok(());
            }
            for p in prefs {
                println!("  {:<26}  {} days", p.device_id, p.days);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(clap::Parser)]
    struct TestCli {
        #[command(flatten)]
        args: Args,
    }

    #[test]
    fn test_no_args_parses() {
        let cli = TestCli::try_parse_from(["test"]).expect("parse");
        assert!(cli.args.device.is_none());
        assert!(cli.args.days.is_none());
    }

    #[test]
    fn test_device_and_days_parse() {
        let cli =
            TestCli::try_parse_from(["test", "--device", "self", "--days", "30"]).expect("parse");
        assert_eq!(cli.args.device.as_deref(), Some("self"));
        assert_eq!(cli.args.days, Some(30));
    }

    #[test]
    fn test_negative_days_parses() {
        // clap requires `--days=-1` (equals form) for negative numbers;
        // semantic validation lives in run().
        let cli = TestCli::try_parse_from(["test", "--days=-1"]).expect("parse");
        assert_eq!(cli.args.days, Some(-1));
    }
}
