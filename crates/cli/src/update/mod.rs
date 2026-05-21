//! Auto-update notifier + self-update subcommand.
//!
//! See `docs/superpowers/specs/2026-05-19-auto-update-design.md` for the
//! full design. `notifier::maybe_notify` runs after each subcommand;
//! `self_update::run` is invoked by the `cinch self-update` subcommand.

pub mod cache;
pub mod manifest;
pub mod notifier;
pub mod self_update;
pub mod source;

use clap::Parser;

#[derive(Parser, Debug)]
pub struct SelfUpdateArgs {
    /// Print what would happen and exit without changing the binary.
    #[arg(long)]
    pub check: bool,
    /// Override the refusal to clobber a package-manager-managed binary.
    #[arg(long)]
    pub force: bool,
}

pub async fn run_self_update(args: SelfUpdateArgs) -> Result<(), crate::exit::ExitError> {
    self_update::run(self_update::RunOptions {
        check_only: args.check,
        force: args.force,
    })
    .await
    .map_err(|e| {
        crate::exit::ExitError::new(crate::exit::GENERIC_ERROR, e.to_string(), String::new())
    })
}
