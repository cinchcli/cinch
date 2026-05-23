//! `cinch clip rm <id-prefix>` — delete a clip from the relay and the local store.

use std::io::{BufRead, IsTerminal};

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// ID prefix (minimum 4 characters).
    pub id: String,
    /// Skip the TTY confirmation prompt.
    #[arg(long)]
    pub force: bool,
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

    if !args.force && std::io::stdin().is_terminal() {
        eprint!("Delete clip {id}? Type 'y' to confirm: ");
        let mut buf = String::new();
        // Treat any I/O failure as empty input — abort rather than panic.
        std::io::stdin().lock().read_line(&mut buf).ok();
        if buf.trim() != "y" {
            eprintln!("aborted");
            return Ok(());
        }
    }

    ctx.client
        .delete_clip(&id)
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("relay: {e}"), ""))?;
    client_core::store::queries::delete_clip(&ctx.store, &id)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("store: {e}"), ""))?;
    println!("deleted {id}");
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
    fn test_force_flag_parses() {
        let cli = TestCli::try_parse_from(["test", "01HXXXXXXX", "--force"]).expect("parse");
        assert_eq!(cli.args.id, "01HXXXXXXX");
        assert!(cli.args.force);
    }

    #[test]
    fn test_force_default_false() {
        let cli = TestCli::try_parse_from(["test", "01HXXXXXXX"]).expect("parse");
        assert_eq!(cli.args.id, "01HXXXXXXX");
        assert!(!cli.args.force);
    }

    #[test]
    fn test_missing_id_errors() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(
            result.is_err(),
            "expected parse failure when <ID> is omitted"
        );
    }
}
