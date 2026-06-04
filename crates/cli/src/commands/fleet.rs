//! `cinch fleet` — manage the machines paired to this account (redesign §2;
//! was `cinch device`).
//!
//! Subcommands:
//! - `fleet list` — list paired machines (and source-only rows). (was `device list`)
//! - `fleet add <SSH-TARGET>` — add a machine over SSH. (was `device pair`)
//! - `fleet rename <DEVICE> [NAME]` — rename a machine; `DEVICE` = `self` or
//!   id-prefix. MERGES `device set-name` (this machine) + `device nickname` (another).
//! - `fleet retention` — view/set per-machine remote retention. (was `device retention`)
//! - `fleet revoke <DEVICE>` — revoke a machine's access. (was `device revoke`)
//! - `fleet sources` — source machines seen in local history. (was `device sources`)
//!
//! Bare `cinch fleet` is `cinch fleet list`.

use client_core::http::HttpError;

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR, NETWORK_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Option<Cmd>,
}

#[derive(Debug, clap::Subcommand)]
pub enum Cmd {
    /// List paired machines for this account.
    List(crate::commands::devices::Args),
    /// Add a machine to the fleet over SSH.
    Add(crate::commands::pair::Args),
    /// Rename a fleet machine. DEVICE is `self` (this machine) or an id-prefix.
    Rename(RenameArgs),
    /// View or set per-machine remote clip retention.
    Retention(crate::commands::retention::Args),
    /// Revoke a machine's access (asks for confirmation).
    Revoke(crate::commands::revoke::Args),
    /// List distinct source machines seen in local history.
    Sources(crate::commands::sources::Args),
}

#[derive(Debug, clap::Args)]
pub struct RenameArgs {
    /// Machine to rename: `self` (this machine) or an id-prefix (min 4 chars).
    pub device: String,
    /// New name; trimmed and capped at 64 bytes UTF-8. Omit with `--clear`.
    pub name: Option<String>,
    /// Clear the name (fall back to the system hostname). Mutually exclusive
    /// with NAME.
    #[arg(long, conflicts_with = "name")]
    pub clear: bool,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        // Bare `cinch fleet` lists the fleet.
        None => {
            crate::commands::devices::run(crate::commands::devices::Args {
                names: false,
                paired_only: false,
            })
            .await
        }
        Some(Cmd::List(a)) => crate::commands::devices::run(a).await,
        Some(Cmd::Add(a)) => crate::commands::pair::run(a).await,
        Some(Cmd::Rename(a)) => run_rename(a.device, a.name, a.clear).await,
        Some(Cmd::Retention(a)) => crate::commands::retention::run(a).await,
        Some(Cmd::Revoke(a)) => crate::commands::revoke::run(a).await,
        Some(Cmd::Sources(a)) => crate::commands::sources::run(a).await,
    }
}

/// Rename a fleet machine. `device == "self"` renames THIS machine (the old
/// `device set-name` behavior — resolves the active device id); any other
/// value is treated as an id-prefix (the old `device nickname` behavior).
///
/// `pub(crate)` so the `device` deprecation shim can route `device set-name`
/// (inject `self`) and `device nickname` here without duplicating the logic.
pub(crate) async fn run_rename(
    device: String,
    name: Option<String>,
    clear: bool,
) -> Result<(), ExitError> {
    let new_name = normalize_name(name, clear)?;

    let ctx = crate::runtime::open_ctx()
        .map_err(|e| ExitError::new(AUTH_FAILURE, e, "Run: cinch auth login"))?;

    let is_self = device.eq_ignore_ascii_case("self");
    let target_id = if is_self {
        if ctx.active_device_id.is_empty() {
            return Err(ExitError::new(
                AUTH_FAILURE,
                "No active device on this machine.",
                "Run: cinch auth login",
            ));
        }
        ctx.active_device_id.clone()
    } else {
        crate::runtime::opportunistic_backfill(&ctx).await;
        client_core::store::prefix::resolve_device_id(&ctx.store, &device)
            .map_err(crate::commands::get::render_resolve_error)?
    };

    ctx.client
        .set_device_nickname(&target_id, &new_name)
        .await
        .map_err(map_rename_error)?;

    // Plane-loud-ish: name which machine changed.
    match (is_self, new_name.is_empty()) {
        (true, true) => eprintln!("\u{2713} Cleared this machine's name (falls back to hostname)."),
        (true, false) => eprintln!("\u{2713} Renamed this machine \u{2192} {new_name}"),
        (false, true) => eprintln!("\u{2713} Cleared name for {target_id}."),
        (false, false) => eprintln!("\u{2713} Renamed {target_id} \u{2192} {new_name}"),
    }
    eprintln!("(refresh with `cinch fleet list` to see the change locally)");
    Ok(())
}

/// Validate the NAME/`--clear` pair into the value sent to the relay. `--clear`
/// → empty string; otherwise trimmed, non-empty, ≤64 bytes.
fn normalize_name(name: Option<String>, clear: bool) -> Result<String, ExitError> {
    if clear {
        return Ok(String::new());
    }
    let raw = name.ok_or_else(|| {
        ExitError::new(
            GENERIC_ERROR,
            "Pass a name or --clear.",
            "Usage: cinch fleet rename <DEVICE> \"My Mac\"   (DEVICE = self | id-prefix)",
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
    Ok(trimmed.to_string())
}

fn map_rename_error(e: HttpError) -> ExitError {
    match e {
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
        other => ExitError::new(GENERIC_ERROR, format!("rename failed: {}", other), ""),
    }
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
    fn bare_fleet_has_no_subcommand() {
        let cli = TestCli::try_parse_from(["test"]).expect("bare fleet parses");
        assert!(cli.args.cmd.is_none());
    }

    #[test]
    fn fleet_list_parses() {
        let cli = TestCli::try_parse_from(["test", "list"]).expect("fleet list parses");
        assert!(matches!(cli.args.cmd, Some(Cmd::List(_))));
    }

    #[test]
    fn fleet_add_parses() {
        let cli = TestCli::try_parse_from(["test", "add", "user@host"]).expect("fleet add parses");
        match cli.args.cmd {
            Some(Cmd::Add(a)) => assert_eq!(a.target, "user@host"),
            _ => panic!("expected Add"),
        }
    }

    #[test]
    fn fleet_rename_self_parses() {
        let cli = TestCli::try_parse_from(["test", "rename", "self", "MyMac"])
            .expect("rename self parses");
        match cli.args.cmd {
            Some(Cmd::Rename(a)) => {
                assert_eq!(a.device, "self");
                assert_eq!(a.name.as_deref(), Some("MyMac"));
                assert!(!a.clear);
            }
            _ => panic!("expected Rename"),
        }
    }

    #[test]
    fn fleet_rename_other_with_clear_parses() {
        let cli = TestCli::try_parse_from(["test", "rename", "01J", "--clear"])
            .expect("rename --clear parses");
        match cli.args.cmd {
            Some(Cmd::Rename(a)) => {
                assert_eq!(a.device, "01J");
                assert!(a.name.is_none());
                assert!(a.clear);
            }
            _ => panic!("expected Rename"),
        }
    }

    #[test]
    fn rename_name_and_clear_conflict() {
        let result = TestCli::try_parse_from(["test", "rename", "self", "MyMac", "--clear"]);
        assert!(result.is_err(), "expected clap to reject name + --clear");
    }

    #[test]
    fn normalize_clear_yields_empty() {
        assert_eq!(normalize_name(None, true).unwrap(), "");
    }

    #[test]
    fn normalize_missing_name_errors() {
        let err = normalize_name(None, false).expect_err("must reject");
        assert_eq!(err.code, GENERIC_ERROR);
    }

    #[test]
    fn normalize_blank_errors() {
        let err = normalize_name(Some("   ".into()), false).expect_err("must reject blank");
        assert_eq!(err.code, GENERIC_ERROR);
    }

    #[test]
    fn normalize_too_long_errors() {
        let err = normalize_name(Some("x".repeat(65)), false).expect_err("must reject too long");
        assert_eq!(err.code, GENERIC_ERROR);
    }

    #[test]
    fn normalize_trims() {
        assert_eq!(
            normalize_name(Some("  Mac  ".into()), false).unwrap(),
            "Mac"
        );
    }
}
