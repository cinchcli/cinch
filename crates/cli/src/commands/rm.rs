//! `cinch history rm <REF>...` — delete clip(s).
//!
//! Cross-plane by default (redesign §1 / eng-review D2): deletes on the fleet
//! (relay) AND in the local store, because "delete everywhere" is the
//! privacy-safe default for an E2EE clipboard. `--local` scopes the delete to
//! the local store only (no relay, no auth required). Now variadic — accepts
//! one or more REFs (id prefixes).

use std::io::{BufRead, IsTerminal};

use client_core::store::{self, Store};

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// One or more clip references (id prefixes, minimum 4 characters each).
    #[arg(required = true)]
    pub ids: Vec<String>,
    /// Skip the TTY confirmation prompt.
    #[arg(long)]
    pub force: bool,
    /// Delete locally only — do not touch the fleet (no relay call, no auth).
    #[arg(long)]
    pub local: bool,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    if args.local {
        run_local(args).await
    } else {
        run_cross_plane(args).await
    }
}

/// Local-only delete: open the store directly, never build a RestClient.
async fn run_local(args: Args) -> Result<(), ExitError> {
    let store_path = store::default_db_path()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store path: {e}"), ""))?;
    let store = Store::open(&store_path)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("open store: {e}"), ""))?;

    let ids = resolve_all(&store, &args.ids)?;
    if !confirm(&ids, args.force, true) {
        eprintln!("aborted");
        return Ok(());
    }

    for id in &ids {
        client_core::store::queries::delete_clip(&store, id)
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
        // Plane-loud (eng-review D2): say this stayed local.
        eprintln!("\u{2713} Deleted {id} locally only");
    }
    Ok(())
}

/// Cross-plane delete: relay + local store.
async fn run_cross_plane(args: Args) -> Result<(), ExitError> {
    let ctx = crate::runtime::open_ctx().map_err(|_| {
        ExitError::new(
            AUTH_FAILURE,
            "No auth token configured.",
            "Run: cinch auth login (or use --local to delete locally only)",
        )
    })?;
    crate::runtime::opportunistic_backfill(&ctx).await;

    let ids = resolve_all(&ctx.store, &args.ids)?;
    if !confirm(&ids, args.force, false) {
        eprintln!("aborted");
        return Ok(());
    }

    for id in &ids {
        ctx.client
            .delete_clip(id)
            .await
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("relay: {e}"), ""))?;
        client_core::store::queries::delete_clip(&ctx.store, id)
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
        // Plane-loud (eng-review D2): the delete crossed to the fleet.
        eprintln!("\u{2713} Deleted {id} on your fleet + locally");
    }
    Ok(())
}

/// Resolve every REF to a full clip id up front, so a bad prefix aborts before
/// any deletion happens (no partial/ambiguous-mid-batch surprises).
fn resolve_all(store: &Store, refs: &[String]) -> Result<Vec<String>, ExitError> {
    refs.iter()
        .map(|r| {
            client_core::store::prefix::resolve_clip_id(store, r)
                .map_err(crate::commands::get::render_resolve_error)
        })
        .collect()
}

/// One TTY confirm covering the whole batch. Returns true to proceed.
fn confirm(ids: &[String], force: bool, local: bool) -> bool {
    if force || !std::io::stdin().is_terminal() {
        return true;
    }
    let scope = if local {
        "locally"
    } else {
        "on the fleet + locally"
    };
    if ids.len() == 1 {
        eprint!("Delete clip {} {scope}? Type 'y' to confirm: ", ids[0]);
    } else {
        eprint!("Delete {} clips {scope}? Type 'y' to confirm: ", ids.len());
    }
    let mut buf = String::new();
    // Treat any I/O failure as empty input — abort rather than panic.
    std::io::stdin().lock().read_line(&mut buf).ok();
    buf.trim() == "y"
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
    fn single_id_parses() {
        let cli = TestCli::try_parse_from(["test", "01HXXXXXXX"]).expect("parse");
        assert_eq!(cli.args.ids, vec!["01HXXXXXXX".to_string()]);
        assert!(!cli.args.force);
        assert!(!cli.args.local);
    }

    #[test]
    fn variadic_ids_parse() {
        let cli = TestCli::try_parse_from(["test", "aaaa", "bbbb", "cccc", "--force"])
            .expect("parse variadic");
        assert_eq!(cli.args.ids, vec!["aaaa", "bbbb", "cccc"]);
        assert!(cli.args.force);
    }

    #[test]
    fn local_flag_parses() {
        let cli = TestCli::try_parse_from(["test", "aaaa", "--local"]).expect("parse");
        assert!(cli.args.local);
    }

    #[test]
    fn missing_id_errors() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(
            result.is_err(),
            "expected parse failure when no REF is given"
        );
    }
}
