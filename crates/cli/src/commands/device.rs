//! `cinch device set-name <name>` — set the current device's display name
//! (nickname). Mirrors the desktop's Customize → Name field; the value
//! appears as the colored pill in the desktop Devices panel and in
//! `cinch devices` output.
//!
//! `cinch auth set-name` is for the user-wide display name (per-account).
//! This command is for the per-device name and operates on the active
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
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::SetName { name, clear } => run_set_name(name, clear).await,
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
        }
    }

    #[test]
    fn set_name_with_name_and_clear_conflicts() {
        let result = TestCli::try_parse_from(["test", "set-name", "MyMac", "--clear"]);
        assert!(result.is_err(), "expected clap to reject name + --clear");
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
