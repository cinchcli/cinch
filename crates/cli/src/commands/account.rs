//! `cinch account` — account plan and usage limits.

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
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::Plan(a) => crate::commands::plan::run(a).await,
    }
}
