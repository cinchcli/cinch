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
    .map_err(to_exit_error)
}

fn to_exit_error(e: self_update::SelfUpdateError) -> crate::exit::ExitError {
    use self_update::SelfUpdateError;
    match e {
        SelfUpdateError::ManagedInstall(ref src) => {
            let kind = match src {
                source::InstallSource::Homebrew => "Homebrew",
                source::InstallSource::Apt { .. } => "apt",
                source::InstallSource::Rpm { .. } => "rpm",
                source::InstallSource::Unknown => "a package manager",
            };
            crate::exit::ExitError::new(
                crate::exit::GENERIC_ERROR,
                format!(
                    "cinch was installed via {}. Self-update is disabled to avoid conflicting with the package manager.",
                    kind
                ),
                format!(
                    "{}\n  (or pass --force to override — this will replace the package-managed binary.)",
                    source::hint(src)
                ),
            )
        }
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
    use crate::update::self_update::SelfUpdateError;
    use crate::update::source::InstallSource;

    #[test]
    fn to_exit_error_managed_homebrew_has_context_and_force_hint() {
        let ee = to_exit_error(SelfUpdateError::ManagedInstall(InstallSource::Homebrew));
        assert!(ee.message.contains("Homebrew"), "msg: {}", ee.message);
        assert!(
            ee.message.contains("Self-update is disabled"),
            "msg: {}",
            ee.message
        );
        assert!(ee.fix.contains("brew upgrade cinch"), "fix: {}", ee.fix);
        assert!(ee.fix.contains("--force"), "fix: {}", ee.fix);
    }

    #[test]
    fn to_exit_error_managed_apt_has_apt_hint() {
        let ee = to_exit_error(SelfUpdateError::ManagedInstall(InstallSource::Apt {
            pkg: "cinch".into(),
        }));
        assert!(ee.message.contains("apt"), "msg: {}", ee.message);
        assert!(ee.fix.contains("apt install"), "fix: {}", ee.fix);
        assert!(ee.fix.contains("--force"), "fix: {}", ee.fix);
    }

    #[test]
    fn to_exit_error_managed_rpm_has_rpm_hint() {
        let ee = to_exit_error(SelfUpdateError::ManagedInstall(InstallSource::Rpm {
            pkg: "cinch-0.5.0-1.x86_64".into(),
        }));
        assert!(ee.message.contains("rpm"), "msg: {}", ee.message);
        assert!(ee.fix.contains("dnf upgrade cinch"), "fix: {}", ee.fix);
        assert!(ee.fix.contains("--force"), "fix: {}", ee.fix);
    }

    #[test]
    fn to_exit_error_fetch_keeps_message_unchanged() {
        let ee = to_exit_error(SelfUpdateError::Fetch("boom".into()));
        assert!(ee.message.contains("boom"), "msg: {}", ee.message);
        assert!(ee.fix.is_empty(), "fix should be empty: {}", ee.fix);
    }
}
