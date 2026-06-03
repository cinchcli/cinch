//! `cinch clip` — DEPRECATED alias group for `cinch history` (redesign §4b).
//!
//! Renamed to `history` in 0.5. Each old `clip <sub>` spelling stays a hidden
//! alias through the 0.8 removal runway: it prints exactly one deprecation
//! note and delegates to the SAME handler the new `history <sub>` uses
//! (`clip get` maps to `history show`). Hidden from help/completions via
//! `#[command(hide = true)]` on the `Clip` variant in `lib.rs`.

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
    /// Delete a clip by ID prefix.
    Rm(crate::commands::rm::Args),
    /// Transform a clip's text content.
    Transform(crate::commands::transform::Args),
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::List(a) => {
            crate::commands::deprecation_note("clip list", "history list");
            crate::commands::list::run(a).await
        }
        Cmd::Search(a) => {
            crate::commands::deprecation_note("clip search", "history search");
            crate::commands::search::run(a).await
        }
        Cmd::Get(a) => {
            // Merge case (§4b): `clip get` → `history show` (single handler);
            // --meta survives because the same `get::Args` is reused.
            crate::commands::deprecation_note("clip get", "history show");
            crate::commands::get::run(a).await
        }
        Cmd::Rm(a) => {
            crate::commands::deprecation_note("clip rm", "history rm");
            crate::commands::rm::run(a).await
        }
        Cmd::Transform(a) => {
            crate::commands::deprecation_note("clip transform", "history transform");
            crate::commands::transform::run(a).await
        }
    }
}
