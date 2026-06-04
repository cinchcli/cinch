//! `cinch device` — DEPRECATED alias group for `cinch fleet` (redesign §4b).
//!
//! Renamed to `fleet` in 0.5. Each old `device <sub>` spelling stays a hidden
//! alias through the 0.8 removal runway: it prints exactly one deprecation
//! note and delegates to the SAME handler the new `fleet <sub>` uses. Hidden
//! from help/completions via `#[command(hide = true)]` on the `Device` variant
//! in `lib.rs`.
//!
//! Two name commands collapse into one (§4b merge case):
//! - `device set-name <NAME>`        → `fleet rename self <NAME>` (self injected)
//! - `device nickname <DEV> <NAME>`  → `fleet rename <DEV> <NAME>`

use crate::exit::ExitError;

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
    /// Set this device's display name (or `--clear`). Targets THIS machine.
    SetName {
        /// New name; trimmed and capped at 64 bytes UTF-8. Mutually exclusive
        /// with `--clear`.
        name: Option<String>,
        /// Clear the name.
        #[arg(long, conflicts_with = "name")]
        clear: bool,
    },
    /// Set or clear another paired device's nickname.
    Nickname(crate::commands::fleet::RenameArgs),
    /// View or set per-device clip retention.
    Retention(crate::commands::retention::Args),
    /// Revoke a paired device's token (asks for confirmation).
    Revoke(crate::commands::revoke::Args),
    /// List distinct source machines that have pushed clips.
    Sources(crate::commands::sources::Args),
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::List(a) => {
            crate::commands::deprecation_note("device list", "fleet list");
            crate::commands::devices::run(a).await
        }
        Cmd::Pair(a) => {
            crate::commands::deprecation_note("device pair", "fleet add");
            crate::commands::pair::run(a).await
        }
        Cmd::SetName { name, clear } => {
            // Merge case (§4b): inject the implicit `self` target.
            crate::commands::deprecation_note("device set-name", "fleet rename self");
            crate::commands::fleet::run_rename("self".to_string(), name, clear).await
        }
        Cmd::Nickname(a) => {
            crate::commands::deprecation_note("device nickname", "fleet rename <DEVICE>");
            crate::commands::fleet::run_rename(a.device, a.name, a.clear).await
        }
        Cmd::Retention(a) => {
            crate::commands::deprecation_note("device retention", "fleet retention");
            crate::commands::retention::run(a).await
        }
        Cmd::Revoke(a) => {
            crate::commands::deprecation_note("device revoke", "fleet revoke");
            crate::commands::revoke::run(a).await
        }
        Cmd::Sources(a) => {
            crate::commands::deprecation_note("device sources", "fleet sources");
            crate::commands::sources::run(a).await
        }
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
    fn parses_nickname_with_device_and_name() {
        // device nickname <DEV> <NAME> reuses fleet::RenameArgs (device, name).
        let cli = TestCli::try_parse_from(["test", "nickname", "01J", "box"])
            .expect("device nickname parses");
        match cli.args.cmd {
            Cmd::Nickname(a) => {
                assert_eq!(a.device, "01J");
                assert_eq!(a.name.as_deref(), Some("box"));
            }
            _ => panic!("expected Nickname"),
        }
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
}
