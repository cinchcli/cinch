//! `cinch device` — manage paired devices on this account.
//!
//! Subcommands:
//! - `cinch device list`                       — list paired devices (and source-only rows).
//! - `cinch device pair <ssh-target>`          — set up cinch on a remote machine via SSH.
//! - `cinch device set-name <name>`            — rename the active device (this machine).
//! - `cinch device nickname <id-prefix> <name>` — rename another paired device.
//! - `cinch device retention [...]`            — view or set per-device retention.
//! - `cinch device revoke <id-prefix>`         — revoke a paired device's token.
//! - `cinch device sources`                    — list distinct source machines that have pushed.
//!
//! `cinch device set-name` mirrors the desktop's Customize → Name field; the value
//! appears as the colored pill in the desktop Devices panel and in `cinch device list`.
//! `cinch auth set-name` is for the user-wide display name (per-account).
//! `cinch device set-name` is for the per-device name and operates on the active
//! device only — no device-id prefix required.

use client_core::auth::load_config;
use client_core::http::{HttpError, RestClient};

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR, NETWORK_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, clap::Subcommand)]
pub enum Cmd {
    /// List paired devices for this account.
    List(crate::commands::devices::Args),
    /// Set up cinch on a remote machine via SSH.
    Pair(crate::commands::pair::Args),
    /// Set this device's display name. Pass `--clear` to remove it and fall
    /// back to the system hostname.
    SetName {
        /// New name; trimmed and capped at 64 bytes UTF-8. Mutually
        /// exclusive with `--clear`.
        name: Option<String>,
        /// Clear the nickname.
        #[arg(long, conflicts_with = "name")]
        clear: bool,
    },
    /// Set or clear another paired device's nickname.
    Nickname(crate::commands::nickname::Args),
    /// View or set per-device clip retention.
    Retention(crate::commands::retention::Args),
    /// Revoke a paired device's token (asks for confirmation).
    Revoke(crate::commands::revoke::Args),
    /// List distinct source machines that have pushed clips.
    Sources(crate::commands::sources::Args),
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::List(a) => crate::commands::devices::run(a).await,
        Cmd::Pair(a) => crate::commands::pair::run(a).await,
        Cmd::SetName { name, clear } => run_set_name(name, clear).await,
        Cmd::Nickname(a) => crate::commands::nickname::run(a).await,
        Cmd::Retention(a) => crate::commands::retention::run(a).await,
        Cmd::Revoke(a) => crate::commands::revoke::run(a).await,
        Cmd::Sources(a) => crate::commands::sources::run(a).await,
    }
}

async fn run_set_name(name: Option<String>, clear: bool) -> Result<(), ExitError> {
    let new_name: String = if clear {
        String::new()
    } else {
        let raw = name.ok_or_else(|| {
            ExitError::new(
                GENERIC_ERROR,
                "Pass a name or --clear.",
                "Usage: cinch device set-name \"My Mac\"",
            )
        })?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(ExitError::new(
                GENERIC_ERROR,
                "Name must not be empty.",
                "Pass a non-empty name or --clear.",
            ));
        }
        if trimmed.len() > 64 {
            return Err(ExitError::new(
                GENERIC_ERROR,
                "Name must be 64 bytes or fewer.",
                "Shorten the name.",
            ));
        }
        trimmed.to_string()
    };

    let cfg = load_config()
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not load config: {}", e), ""))?;
    if cfg.token.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "Not authenticated.",
            "Run: cinch auth login",
        ));
    }
    if cfg.active_device_id.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "No active device on this machine.",
            "Run: cinch auth login",
        ));
    }

    let client = RestClient::new(
        cfg.relay_url.clone(),
        cfg.token.clone(),
        crate::client_info::for_cli(),
    )
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not init client: {}", e), ""))?;

    client
        .set_device_nickname(&cfg.active_device_id, &new_name)
        .await
        .map_err(|e| match e {
            HttpError::Unauthorized => ExitError::new(
                AUTH_FAILURE,
                "Authentication failed.",
                "Run: cinch auth logout && cinch auth login",
            ),
            HttpError::Relay {
                status: 400,
                message,
                ..
            } => ExitError::new(GENERIC_ERROR, message, ""),
            HttpError::Network(msg) => ExitError::new(
                NETWORK_ERROR,
                "Relay unreachable.",
                format!("Check your connection or try again later. ({})", msg),
            ),
            other => ExitError::new(GENERIC_ERROR, format!("set-name failed: {}", other), ""),
        })?;

    if new_name.is_empty() {
        eprintln!("\u{2713} Cleared device name (will fall back to hostname).");
    } else {
        eprintln!("\u{2713} Device name updated: {}", new_name);
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
    fn parses_set_name_with_value() {
        let cli =
            TestCli::try_parse_from(["test", "set-name", "MyMac"]).expect("set-name MyMac parses");
        match cli.args.cmd {
            Cmd::SetName { name, clear } => {
                assert_eq!(name.as_deref(), Some("MyMac"));
                assert!(!clear);
            }
            _ => panic!("expected SetName"),
        }
    }

    #[test]
    fn parses_set_name_clear() {
        let cli = TestCli::try_parse_from(["test", "set-name", "--clear"])
            .expect("set-name --clear parses");
        match cli.args.cmd {
            Cmd::SetName { name, clear } => {
                assert!(name.is_none());
                assert!(clear);
            }
            _ => panic!("expected SetName"),
        }
    }

    #[test]
    fn set_name_with_name_and_clear_conflicts() {
        let result = TestCli::try_parse_from(["test", "set-name", "MyMac", "--clear"]);
        assert!(result.is_err(), "expected clap to reject name + --clear");
    }

    #[test]
    fn parses_list_subcommand() {
        let cli = TestCli::try_parse_from(["test", "list"]).expect("device list parses");
        assert!(matches!(cli.args.cmd, Cmd::List(_)));
    }

    #[test]
    fn parses_pair_subcommand() {
        let cli =
            TestCli::try_parse_from(["test", "pair", "user@host"]).expect("device pair parses");
        match cli.args.cmd {
            Cmd::Pair(a) => assert_eq!(a.target, "user@host"),
            _ => panic!("expected Pair"),
        }
    }

    #[tokio::test]
    async fn run_set_name_rejects_missing_args() {
        // Both name=None and clear=false → error.
        let err = run_set_name(None, false).await.expect_err("must reject");
        assert_eq!(err.code, GENERIC_ERROR);
    }

    #[tokio::test]
    async fn run_set_name_rejects_blank() {
        let err = run_set_name(Some("   ".into()), false)
            .await
            .expect_err("must reject blank");
        assert_eq!(err.code, GENERIC_ERROR);
    }

    #[tokio::test]
    async fn run_set_name_rejects_too_long() {
        let long = "x".repeat(65);
        let err = run_set_name(Some(long), false)
            .await
            .expect_err("must reject too long");
        assert_eq!(err.code, GENERIC_ERROR);
    }
}
