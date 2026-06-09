//! Package-manager upgrade dispatch (brew/apt/rpm), mockable for tests.

use crate::update::source::InstallSource;
use std::io;
use std::process::{Command, ExitStatus};

/// The `(program, args)` to upgrade a managed install. `None` for `Unknown`,
/// which is handled by the direct binary swap, not a package manager.
pub fn upgrade_command(source: &InstallSource) -> Option<(String, Vec<String>)> {
    let v = |xs: &[&str]| xs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match source {
        InstallSource::Homebrew { cask: false } => {
            Some(("brew".into(), v(&["upgrade", "cinchcli"])))
        }
        InstallSource::Homebrew { cask: true } => {
            Some(("brew".into(), v(&["upgrade", "--cask", "cinchcli"])))
        }
        InstallSource::Apt { pkg } => Some((
            "sudo".into(),
            v(&["apt-get", "install", "--only-upgrade", "-y", pkg]),
        )),
        InstallSource::Rpm { pkg } => Some((
            "sudo".into(),
            v(&["dnf", "upgrade", "-y", &rpm_base_name(pkg)]),
        )),
        InstallSource::Unknown => None,
    }
}

/// `rpm -qf` returns NVRA (`cinch-0.5.0-1.x86_64`); dnf wants the package name.
/// Cut at the first `-` directly followed by a digit; otherwise return as-is.
fn rpm_base_name(nvra: &str) -> String {
    let bytes = nvra.as_bytes();
    for i in 1..bytes.len() {
        if bytes[i - 1] == b'-' && bytes[i].is_ascii_digit() {
            return nvra[..i - 1].to_string();
        }
    }
    nvra.to_string()
}

/// Human-readable command for messages/errors.
pub fn command_string(source: &InstallSource) -> String {
    match upgrade_command(source) {
        Some((p, a)) => format!("{} {}", p, a.join(" ")),
        None => "cinch update --force".to_string(),
    }
}

/// Mockable boundary so dispatch is unit-testable without shelling out.
pub trait PackageManagerRunner {
    fn upgrade(&self, source: &InstallSource) -> io::Result<ExitStatus>;
}

/// Production runner: inherits stdio so `sudo` can prompt on the terminal and
/// brew/apt output streams live.
pub struct RealRunner;

impl PackageManagerRunner for RealRunner {
    fn upgrade(&self, source: &InstallSource) -> io::Result<ExitStatus> {
        let (prog, args) = upgrade_command(source).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "no package-manager command for this source",
            )
        })?;
        Command::new(prog).args(args).status()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::source::InstallSource;

    #[test]
    fn brew_formula_command() {
        let (p, a) = upgrade_command(&InstallSource::Homebrew { cask: false }).unwrap();
        assert_eq!(p, "brew");
        assert_eq!(a, vec!["upgrade", "cinchcli"]);
    }

    #[test]
    fn brew_cask_command() {
        let (p, a) = upgrade_command(&InstallSource::Homebrew { cask: true }).unwrap();
        assert_eq!(p, "brew");
        assert_eq!(a, vec!["upgrade", "--cask", "cinchcli"]);
    }

    #[test]
    fn apt_command_uses_sudo_and_pkg() {
        let (p, a) = upgrade_command(&InstallSource::Apt {
            pkg: "cinch".into(),
        })
        .unwrap();
        assert_eq!(p, "sudo");
        assert_eq!(
            a,
            vec!["apt-get", "install", "--only-upgrade", "-y", "cinch"]
        );
    }

    #[test]
    fn rpm_command_uses_base_name() {
        let (p, a) = upgrade_command(&InstallSource::Rpm {
            pkg: "cinch-0.5.0-1.x86_64".into(),
        })
        .unwrap();
        assert_eq!(p, "sudo");
        assert_eq!(a, vec!["dnf", "upgrade", "-y", "cinch"]);
    }

    #[test]
    fn unknown_has_no_command() {
        assert!(upgrade_command(&InstallSource::Unknown).is_none());
    }

    #[test]
    fn rpm_base_name_strips_nvra() {
        assert_eq!(rpm_base_name("cinch-0.5.0-1.x86_64"), "cinch");
        assert_eq!(rpm_base_name("cinch"), "cinch");
        assert_eq!(rpm_base_name("my-tool-1.0-1"), "my-tool");
    }

    #[test]
    fn command_string_is_human_readable() {
        let s = command_string(&InstallSource::Homebrew { cask: true });
        assert_eq!(s, "brew upgrade --cask cinchcli");
        assert_eq!(
            command_string(&InstallSource::Unknown),
            "cinch update --force"
        );
    }
}
