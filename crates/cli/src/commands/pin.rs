//! `cinch pin` — pin/unpin clips and list pinned clips.
//!
//! Subcommands:
//! - `cinch pin add <id-prefix>`    — pin a clip locally and on the relay.
//! - `cinch pin rm <id-prefix>`     — unpin a clip locally and on the relay.
//! - `cinch pin list`               — list pinned clips (alias of `clip list --pinned`).

use std::time::{SystemTime, UNIX_EPOCH};

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, clap::Subcommand)]
pub enum Cmd {
    /// Pin a clip by ID prefix.
    Add(AddArgs),
    /// Unpin a clip by ID prefix.
    Rm(RmArgs),
    /// List pinned clips.
    List(ListArgs),
}

#[derive(Debug, clap::Args)]
pub struct AddArgs {
    /// ID prefix (minimum 4 characters).
    pub id: String,
}

#[derive(Debug, clap::Args)]
pub struct RmArgs {
    /// ID prefix (minimum 4 characters).
    pub id: String,
}

#[derive(Debug, clap::Args)]
pub struct ListArgs {
    /// Max number of clips to return. Hard cap is 200.
    #[arg(long, default_value_t = 50)]
    pub limit: u32,
    /// Force JSON output (default when stdout is not a TTY).
    #[arg(long)]
    pub json: bool,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::Add(a) => run_add(a).await,
        Cmd::Rm(a) => run_rm(a).await,
        Cmd::List(a) => run_list(a).await,
    }
}

async fn run_add(args: AddArgs) -> Result<(), ExitError> {
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
        .set_clip_pin(&id, true, None)
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("relay: {e}"), ""))?;
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;
    client_core::store::queries::set_pinned(&ctx.store, &id, true, now_ms)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
    println!("pinned {id}");
    Ok(())
}

async fn run_rm(args: RmArgs) -> Result<(), ExitError> {
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

async fn run_list(args: ListArgs) -> Result<(), ExitError> {
    let list_args = crate::commands::list::Args {
        limit: args.limit,
        from: None,
        text_only: false,
        exclude_self: false,
        json: args.json,
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
    fn pin_add_parses() {
        let cli = TestCli::try_parse_from(["test", "add", "abcd"]).expect("pin add parses");
        match cli.args.cmd {
            Cmd::Add(a) => assert_eq!(a.id, "abcd"),
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn pin_rm_parses() {
        let cli = TestCli::try_parse_from(["test", "rm", "abcd"]).expect("pin rm parses");
        match cli.args.cmd {
            Cmd::Rm(a) => assert_eq!(a.id, "abcd"),
            _ => panic!("expected Rm"),
        }
    }

    #[test]
    fn pin_list_parses() {
        let cli = TestCli::try_parse_from(["test", "list", "--limit", "10", "--json"])
            .expect("pin list parses");
        match cli.args.cmd {
            Cmd::List(a) => {
                assert_eq!(a.limit, 10);
                assert!(a.json);
            }
            _ => panic!("expected List"),
        }
    }

    #[test]
    fn list_args_field_parity_with_list_module() {
        // Compile-time test: ensure run_list's explicit construction of
        // list::Args stays in sync. If a new required field is added to
        // list::Args, this must be updated.
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
