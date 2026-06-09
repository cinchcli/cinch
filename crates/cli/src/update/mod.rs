//! Auto-update notifier + the `cinch update` subcommand.
//!
//! See `docs/superpowers/specs/2026-05-19-auto-update-design.md` for the
//! full design. `notifier::maybe_notify` runs after each subcommand;
//! `self_update::run` is invoked by the `cinch update` subcommand (the
//! former `cinch self-update`, now removed and routed to a hard error).

pub mod cache;
pub mod manifest;
pub mod notifier;
pub mod pm;
pub mod self_update;
pub mod source;

use clap::Parser;

#[derive(Parser, Debug)]
pub struct UpdateArgs {
    /// Report the available version and exit without changing anything.
    #[arg(long)]
    pub check: bool,
    /// Skip the confirmation prompt (required when stdin is not a TTY).
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Bypass the package manager and replace the binary with a direct
    /// download, even on a managed (brew/apt/rpm) install.
    #[arg(long)]
    pub force: bool,
}

/// Catch-all args for the removed `self-update` spelling (mirrors `push`):
/// any flags/args still route to the hard-error redirect.
#[derive(clap::Args, Debug)]
pub struct RemovedArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
    pub _rest: Vec<String>,
}

pub async fn run_update(args: UpdateArgs) -> Result<(), crate::exit::ExitError> {
    self_update::run(
        self_update::RunOptions {
            check_only: args.check,
            yes: args.yes,
            force: args.force,
        },
        &pm::RealRunner,
    )
    .await
    .map_err(to_exit_error)
}

/// `cinch self-update` was renamed to `cinch update`. Hard error, does nothing.
pub async fn run_removed_self_update() -> Result<(), crate::exit::ExitError> {
    Err(crate::exit::ExitError::new(
        crate::exit::GENERIC_ERROR,
        "`cinch self-update` was renamed to `cinch update`.",
        "Run: cinch update",
    ))
}

fn to_exit_error(e: self_update::UpdateError) -> crate::exit::ExitError {
    use self_update::UpdateError;
    match e {
        UpdateError::NeedsConfirmation { from, to } => crate::exit::ExitError::new(
            crate::exit::GENERIC_ERROR,
            format!("A new version of cinch is available: {} → {}.", from, to),
            "Run: cinch update --yes",
        ),
        UpdateError::PackageManager { cmd, detail } => crate::exit::ExitError::new(
            crate::exit::GENERIC_ERROR,
            format!("Update via the package manager failed: {}", detail),
            format!("Run it manually: {}", cmd),
        ),
        other => crate::exit::ExitError::new(
            crate::exit::GENERIC_ERROR,
            other.to_string(),
            String::new(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::self_update::UpdateError;

    #[test]
    fn to_exit_error_fetch_keeps_message_unchanged() {
        let ee = to_exit_error(UpdateError::Fetch("boom".into()));
        assert!(ee.message.contains("boom"), "msg: {}", ee.message);
        assert!(ee.fix.is_empty(), "fix should be empty: {}", ee.fix);
    }

    #[test]
    fn to_exit_error_needs_confirmation_points_at_yes() {
        let ee = to_exit_error(self_update::UpdateError::NeedsConfirmation {
            from: "0.7.0".into(),
            to: "0.8.3".into(),
        });
        assert_eq!(ee.code, crate::exit::GENERIC_ERROR);
        assert!(ee.fix.contains("cinch update --yes"), "fix: {}", ee.fix);
    }

    #[test]
    fn to_exit_error_package_manager_repeats_command() {
        let ee = to_exit_error(self_update::UpdateError::PackageManager {
            cmd: "brew upgrade cinchcli".into(),
            detail: "exited with 1".into(),
        });
        assert!(ee.fix.contains("brew upgrade cinchcli"), "fix: {}", ee.fix);
    }

    #[tokio::test]
    async fn removed_self_update_hard_errors_to_update() {
        let ee = run_removed_self_update()
            .await
            .expect_err("must hard-error");
        assert_eq!(ee.code, crate::exit::GENERIC_ERROR);
        let combined = format!("{} {}", ee.message, ee.fix);
        assert!(combined.contains("cinch update"), "got: {combined}");
    }
}
