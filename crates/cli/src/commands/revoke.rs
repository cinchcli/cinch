//! `cinch device revoke <device-id-prefix> [--force]` — revoke a paired
//! device's token. Self-revoke requires uppercase `YES` to confirm.

use std::io::{BufRead, IsTerminal};

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Device ID prefix (minimum 4 characters).
    pub device: String,
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

    let id = client_core::store::prefix::resolve_device_id(&ctx.store, &args.device)
        .map_err(crate::commands::get::render_resolve_error)?;

    let self_id = client_core::auth::load_config()
        .ok()
        .map(|c| c.active_device_id)
        .filter(|s| !s.is_empty());
    let is_self = self_id.as_deref() == Some(id.as_str());

    if !args.force && std::io::stdin().is_terminal() {
        if is_self {
            eprint!("Warning: this is THIS DEVICE. Type 'YES' to confirm: ");
        } else {
            eprint!("Revoke device {id}? Type 'y' to confirm: ");
        }
        let mut buf = String::new();
        // Treat any I/O failure as empty input — abort rather than panic.
        std::io::stdin().lock().read_line(&mut buf).ok();
        let expected = if is_self { "YES" } else { "y" };
        if buf.trim() != expected {
            eprintln!("aborted");
            return Ok(());
        }
    }

    ctx.client
        .revoke_device(&id)
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("relay: {e}"), ""))?;
    println!("revoked {id}");
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
    fn test_device_required() {
        let result = TestCli::try_parse_from(["test"]);
        assert!(
            result.is_err(),
            "expected parse failure when <DEVICE> omitted"
        );
    }

    #[test]
    fn test_force_flag_parses() {
        let cli = TestCli::try_parse_from(["test", "01J...", "--force"]).expect("parse");
        assert_eq!(cli.args.device, "01J...");
        assert!(cli.args.force);
    }

    #[test]
    fn test_force_default_false() {
        let cli = TestCli::try_parse_from(["test", "01J..."]).expect("parse");
        assert!(!cli.args.force);
    }
}
