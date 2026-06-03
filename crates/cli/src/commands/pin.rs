//! `cinch pin <REF>` — pin a clip (redesign §2; was `pin add`).
//!
//! Cross-plane by default (eng-review D2): pins on the fleet (relay) AND
//! locally. `--local` scopes it to the local store only. The counterpart
//! `cinch unpin <REF>` lives in `unpin.rs`; both share [`set_pin`].
//!
//! The pre-0.5 group forms (`pin add` / `pin rm` / `pin list`) survive as
//! hidden subcommands that print one deprecation note and route to the new
//! behavior, through the 0.8 removal runway.

use std::time::{SystemTime, UNIX_EPOCH};

use client_core::store::{self, Store};

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct Args {
    /// Deprecated pre-0.5 subforms (`pin add/rm/list`). Hidden; route to the
    /// new behavior with a one-line deprecation note.
    #[command(subcommand)]
    pub legacy: Option<Legacy>,

    /// Clip reference (id prefix, min 4 chars) to pin. Cross-plane (fleet +
    /// local) unless `--local`.
    pub reference: Option<String>,

    /// Pin locally only — do not touch the fleet (no relay call, no auth).
    #[arg(long)]
    pub local: bool,
}

#[derive(Debug, clap::Subcommand)]
pub enum Legacy {
    /// (deprecated) `pin add <REF>` → `pin <REF>`.
    #[command(hide = true)]
    Add(LegacyPinArgs),
    /// (deprecated) `pin rm <REF>` → `unpin <REF>`.
    #[command(hide = true)]
    Rm(LegacyPinArgs),
    /// (deprecated) `pin list` → `history list --pinned`.
    #[command(hide = true)]
    List(LegacyListArgs),
}

#[derive(Debug, clap::Args)]
pub struct LegacyPinArgs {
    /// ID prefix (minimum 4 characters).
    pub reference: String,
    /// Pin/unpin locally only.
    #[arg(long)]
    pub local: bool,
}

#[derive(Debug, clap::Args)]
pub struct LegacyListArgs {
    /// Max number of clips to return. Hard cap is 200.
    #[arg(long, default_value_t = 50)]
    pub limit: u32,
    /// Force JSON output (default when stdout is not a TTY).
    #[arg(long)]
    pub json: bool,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    if let Some(legacy) = args.legacy {
        return match legacy {
            Legacy::Add(a) => {
                crate::commands::deprecation_note("pin add", "pin");
                set_pin(&a.reference, a.local, true).await
            }
            Legacy::Rm(a) => {
                crate::commands::deprecation_note("pin rm", "unpin");
                set_pin(&a.reference, a.local, false).await
            }
            Legacy::List(a) => {
                crate::commands::deprecation_note("pin list", "history list --pinned");
                run_pinned_list(a.limit, a.json).await
            }
        };
    }

    let reference = args.reference.ok_or_else(|| {
        ExitError::new(
            GENERIC_ERROR,
            "Pass a clip reference to pin.",
            "Usage: cinch pin <REF>   (REF = id prefix)",
        )
    })?;
    set_pin(&reference, args.local, true).await
}

/// Shared pin/unpin core for `pin`, `unpin`, and the deprecated subforms.
///
/// `pinned = true` pins, `false` unpins. `local` keeps it to the local store
/// only (no relay, no auth); otherwise it is cross-plane (fleet + local) and
/// the success output says which plane(s) it touched.
pub(crate) async fn set_pin(reference: &str, local: bool, pinned: bool) -> Result<(), ExitError> {
    let verb = if pinned { "Pinned" } else { "Unpinned" };
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    if local {
        let store_path = store::default_db_path()
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store path: {e}"), ""))?;
        let store = Store::open(&store_path)
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("open store: {e}"), ""))?;
        let id = client_core::store::prefix::resolve_clip_id(&store, reference)
            .map_err(crate::commands::get::render_resolve_error)?;
        client_core::store::queries::set_pinned(&store, &id, pinned, now_ms)
            .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
        // Plane-loud (eng-review D2).
        eprintln!("\u{2713} {verb} {id} locally only");
        return Ok(());
    }

    let ctx = crate::runtime::open_ctx().map_err(|_| {
        ExitError::new(
            AUTH_FAILURE,
            "No auth token configured.",
            "Run: cinch auth login (or use --local to pin locally only)",
        )
    })?;
    crate::runtime::opportunistic_backfill(&ctx).await;

    let id = client_core::store::prefix::resolve_clip_id(&ctx.store, reference)
        .map_err(crate::commands::get::render_resolve_error)?;
    ctx.client
        .set_clip_pin(&id, pinned, None)
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("relay: {e}"), ""))?;
    // when_ms is ignored by the query when pinned=false (column set to NULL).
    client_core::store::queries::set_pinned(&ctx.store, &id, pinned, now_ms)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
    // Plane-loud (eng-review D2): the action crossed to the fleet.
    eprintln!("\u{2713} {verb} {id} on your fleet + locally");
    Ok(())
}

/// `pin list` legacy path → `history list --pinned`.
async fn run_pinned_list(limit: u32, json: bool) -> Result<(), ExitError> {
    let list_args = crate::commands::list::Args {
        limit,
        from: None,
        text_only: false,
        exclude_self: false,
        json,
        remote: false,
        pinned: true,
    };
    crate::commands::list::run(list_args).await
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
    fn new_pin_ref_parses() {
        let cli = TestCli::try_parse_from(["test", "abcd"]).expect("pin <REF> parses");
        assert!(cli.args.legacy.is_none());
        assert_eq!(cli.args.reference.as_deref(), Some("abcd"));
        assert!(!cli.args.local);
    }

    #[test]
    fn new_pin_ref_local_parses() {
        let cli = TestCli::try_parse_from(["test", "abcd", "--local"]).expect("pin <REF> --local");
        assert_eq!(cli.args.reference.as_deref(), Some("abcd"));
        assert!(cli.args.local);
    }

    #[test]
    fn legacy_pin_add_parses_to_legacy_add() {
        let cli = TestCli::try_parse_from(["test", "add", "abcd"]).expect("pin add parses");
        match cli.args.legacy {
            Some(Legacy::Add(a)) => assert_eq!(a.reference, "abcd"),
            _ => panic!("expected Legacy::Add"),
        }
    }

    #[test]
    fn legacy_pin_rm_parses_to_legacy_rm() {
        let cli = TestCli::try_parse_from(["test", "rm", "abcd"]).expect("pin rm parses");
        assert!(matches!(cli.args.legacy, Some(Legacy::Rm(_))));
    }

    #[test]
    fn legacy_pin_list_parses_to_legacy_list() {
        let cli =
            TestCli::try_parse_from(["test", "list", "--limit", "10", "--json"]).expect("pin list");
        match cli.args.legacy {
            Some(Legacy::List(a)) => {
                assert_eq!(a.limit, 10);
                assert!(a.json);
            }
            _ => panic!("expected Legacy::List"),
        }
    }

    #[test]
    fn list_args_field_parity_with_list_module() {
        // Compile-time guard: if list::Args gains a required field, update
        // run_pinned_list's explicit construction.
        let _ = crate::commands::list::Args {
            limit: 10,
            from: None,
            text_only: false,
            exclude_self: false,
            json: false,
            remote: false,
            pinned: true,
        };
    }
}
