//! `cinch clip` — operations on individual clips and the local clip history.
//!
//! Each subcommand re-exports an existing module's `Args` so behavior is
//! identical to the previous top-level commands (`cinch list`, `cinch search`,
//! `cinch get`, `cinch rm`).

use crate::exit::ExitError;

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, clap::Subcommand)]
pub enum Cmd {
    /// List recent clips.
    List(crate::commands::list::Args),
    /// Full-text search across the local clip store.
    Search(crate::commands::search::Args),
    /// Print a single clip's content by ID prefix.
    Get(crate::commands::get::Args),
    /// Delete a clip by ID prefix (with TTY confirm unless --force).
    Rm(crate::commands::rm::Args),
    /// Transform a clip's text content.
    Transform(crate::commands::transform::Args),
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::List(a) => crate::commands::list::run(a).await,
        Cmd::Search(a) => crate::commands::search::run(a).await,
        Cmd::Get(a) => crate::commands::get::run(a).await,
        Cmd::Rm(a) => crate::commands::rm::run(a).await,
        Cmd::Transform(a) => crate::commands::transform::run(a).await,
    }
}
