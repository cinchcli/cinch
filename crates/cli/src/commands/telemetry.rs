//! `cinch account telemetry` — view or change anonymous usage telemetry state.
//!
//! Telemetry is opt-IN (off by default). `on` creates `~/.cinch/telemetry_opt_in`;
//! `off` removes it. When enabled, events go to the user's OWN active relay at
//! `POST {relay_url}/telemetry/otlp`.

use crate::exit::{ExitError, GENERIC_ERROR};
use crate::telemetry;

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, clap::Subcommand)]
pub enum Cmd {
    /// Show current telemetry state and how it's configured.
    Status,
    /// Enable telemetry on this machine (creates the opt-in file).
    On,
    /// Disable telemetry on this machine (removes the opt-in file).
    Off,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::Status => {
            let s = telemetry::status();
            println!("active:       {}", yesno(s.active));
            println!("opted in:     {}", yesno(s.opted_in));
            println!(
                "destination:  {}",
                s.destination.as_deref().unwrap_or("none configured")
            );
            println!("TELEMETRY_DISABLED env: {}", yesno(s.env_disabled));
            println!("DO_NOT_TRACK env:       {}", yesno(s.do_not_track));
            println!();
            println!("Details: https://cinchcli.com/telemetry");
            Ok(())
        }
        Cmd::On => {
            telemetry::set_opt_in(true).map_err(|e| {
                ExitError::new(
                    GENERIC_ERROR,
                    format!("Could not write opt-in file: {}", e),
                    String::new(),
                )
            })?;
            eprintln!("\u{2713} Telemetry enabled.");
            match telemetry::status().destination {
                Some(host) => eprintln!("  Events will be sent to your relay: {}", host),
                None => eprintln!(
                    "  No relay configured yet, so nothing will be sent until you sign in (cinch auth login)."
                ),
            }
            Ok(())
        }
        Cmd::Off => {
            telemetry::set_opt_in(false).map_err(|e| {
                ExitError::new(
                    GENERIC_ERROR,
                    format!("Could not remove opt-in file: {}", e),
                    String::new(),
                )
            })?;
            eprintln!("\u{2713} Telemetry disabled.");
            Ok(())
        }
    }
}

fn yesno(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    // Wrap Args in a parser shell so we can drive subcommand parsing the
    // same way clap does at runtime (`cinch account telemetry status` etc.) without
    // going through the top-level CLI.
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(flatten)]
        args: Args,
    }

    #[test]
    fn yesno_returns_yes_for_true_and_no_for_false() {
        assert_eq!(yesno(true), "yes");
        assert_eq!(yesno(false), "no");
    }

    #[test]
    fn parses_status_subcommand() {
        let cli = TestCli::try_parse_from(["test", "status"]).expect("status parses");
        assert!(matches!(cli.args.cmd, Cmd::Status));
    }

    #[test]
    fn parses_on_subcommand() {
        let cli = TestCli::try_parse_from(["test", "on"]).expect("on parses");
        assert!(matches!(cli.args.cmd, Cmd::On));
    }

    #[test]
    fn parses_off_subcommand() {
        let cli = TestCli::try_parse_from(["test", "off"]).expect("off parses");
        assert!(matches!(cli.args.cmd, Cmd::Off));
    }

    #[test]
    fn rejects_unknown_subcommand() {
        // Guard against a future rename silently changing the public CLI:
        // `cinch account telemetry enable` (or any other word) must NOT parse — it
        // has to fail loudly so users see the help text.
        let err = TestCli::try_parse_from(["test", "enable"]).expect_err("unknown rejects");
        let rendered = format!("{}", err);
        assert!(
            rendered.contains("unrecognized") || rendered.contains("invalid"),
            "expected clap to reject unknown subcommand; got: {rendered}"
        );
    }
}
