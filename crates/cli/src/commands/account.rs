//! `cinch account` — account-level commands: plan tier + telemetry preference.
//!
//! Each subcommand delegates to its previous top-level module so behavior is
//! identical to the old `cinch plan` / `cinch telemetry` entry points.

use crate::exit::ExitError;

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, clap::Subcommand)]
pub enum Cmd {
    /// Show your plan tier and current usage (devices, retention cap).
    Plan(crate::commands::plan::Args),
    /// View or change anonymous usage telemetry state.
    Telemetry(crate::commands::telemetry::Args),
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::Plan(a) => crate::commands::plan::run(a).await,
        Cmd::Telemetry(a) => crate::commands::telemetry::run(a).await,
    }
}
