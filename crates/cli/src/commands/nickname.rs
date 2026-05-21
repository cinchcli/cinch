//! `cinch nickname <device-id-prefix> <name>` / `--clear` — set or unset a
//! device's nickname.

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Device ID prefix (minimum 4 characters).
    pub device: String,
    /// New nickname. Mutually exclusive with `--clear`.
    pub name: Option<String>,
    /// Clear the device's nickname.
    #[arg(long, conflicts_with = "name")]
    pub clear: bool,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    if args.name.is_none() && !args.clear {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "provide a name or --clear",
            "Usage: cinch nickname <device-id-prefix> <name>",
        ));
    }

    let ctx = crate::runtime::open_ctx().map_err(|_| {
        ExitError::new(
            AUTH_FAILURE,
            "No auth token configured.",
            "Run: cinch auth login",
        )
    })?;
    crate::runtime::opportunistic_backfill(&ctx).await;

    let device_id = client_core::store::prefix::resolve_device_id(&ctx.store, &args.device)
        .map_err(crate::commands::get::render_resolve_error)?;

    let new_nickname: &str = if args.clear {
        ""
    } else {
        // Guaranteed Some by the (None, !clear) guard at the top of run().
        args.name
            .as_deref()
            .expect("name is Some when clear is false")
    };
    ctx.client
        .set_device_nickname(&device_id, new_nickname)
        .await
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("relay: {e}"), ""))?;

    if args.clear {
        println!("cleared nickname for {device_id}");
    } else {
        println!("renamed {device_id} → {new_nickname}");
    }
    eprintln!("(refresh with `cinch devices` to see the change locally)");
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
    fn test_name_form_parses() {
        let cli = TestCli::try_parse_from(["test", "01J...", "mybox"]).expect("parse");
        assert_eq!(cli.args.device, "01J...");
        assert_eq!(cli.args.name.as_deref(), Some("mybox"));
        assert!(!cli.args.clear);
    }

    #[test]
    fn test_clear_flag_parses() {
        let cli = TestCli::try_parse_from(["test", "01J...", "--clear"]).expect("parse");
        assert_eq!(cli.args.device, "01J...");
        assert!(cli.args.name.is_none());
        assert!(cli.args.clear);
    }

    #[test]
    fn test_name_and_clear_conflicts() {
        let result = TestCli::try_parse_from(["test", "01J...", "mybox", "--clear"]);
        assert!(result.is_err(), "expected clap to reject name + --clear");
    }
}
