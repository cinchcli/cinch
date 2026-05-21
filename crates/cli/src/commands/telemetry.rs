//! `cinch telemetry` — view or change anonymous usage telemetry state.

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
    /// Enable telemetry on this machine (removes the opt-out file).
    On,
    /// Disable telemetry on this machine (creates the opt-out file).
    Off,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::Status => {
            let s = telemetry::status();
            println!("active:        {}", if s.active { "yes" } else { "no" });
            println!(
                "compiled in:   {}",
                if s.compiled_in {
                    "yes"
                } else {
                    "no (self-built binary — telemetry is permanently disabled)"
                }
            );
            println!("TELEMETRY_DISABLED env: {}", yesno(s.env_disabled));
            println!("DO_NOT_TRACK env:       {}", yesno(s.do_not_track));
            println!("~/.cinch/telemetry_opt_out file: {}", yesno(s.opt_out_file));
            println!();
            println!("Details: https://cinchcli.com/telemetry");
            Ok(())
        }
        Cmd::On => {
            telemetry::set_opt_out(false).map_err(|e| {
                ExitError::new(
                    GENERIC_ERROR,
                    format!("Could not remove opt-out file: {}", e),
                    String::new(),
                )
            })?;
            eprintln!("\u{2713} Telemetry enabled.");
            if !telemetry::status().compiled_in {
                eprintln!("  Note: this binary was built without a telemetry key, so events still won't be sent.");
            }
            Ok(())
        }
        Cmd::Off => {
            telemetry::set_opt_out(true).map_err(|e| {
                ExitError::new(
                    GENERIC_ERROR,
                    format!("Could not write opt-out file: {}", e),
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
