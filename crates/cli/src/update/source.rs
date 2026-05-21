//! Detect how the current `cinch` binary was installed.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallSource {
    Homebrew,
    Apt { pkg: String },
    Rpm { pkg: String },
    Unknown,
}

/// Mockable boundary for the three shell-outs. Production: `RealDetector`.
pub trait Detector {
    fn brew_prefix(&self) -> Option<PathBuf>;
    fn dpkg_owner(&self, exe: &Path) -> Option<String>;
    fn rpm_owner(&self, exe: &Path) -> Option<String>;
}

pub fn detect(exe: &Path, d: &dyn Detector) -> InstallSource {
    if let Some(prefix) = d.brew_prefix() {
        if exe.starts_with(&prefix) {
            return InstallSource::Homebrew;
        }
    }
    if let Some(pkg) = d.dpkg_owner(exe) {
        if pkg.starts_with("cinch") {
            return InstallSource::Apt { pkg };
        }
    }
    if let Some(pkg) = d.rpm_owner(exe) {
        if pkg.starts_with("cinch") {
            return InstallSource::Rpm { pkg };
        }
    }
    InstallSource::Unknown
}

pub fn hint(source: &InstallSource) -> &'static str {
    match source {
        InstallSource::Homebrew => "Run: brew upgrade cinch",
        InstallSource::Apt { .. } => {
            "Run: sudo apt update && sudo apt install --only-upgrade cinch"
        }
        InstallSource::Rpm { .. } => "Run: sudo dnf upgrade cinch",
        InstallSource::Unknown => "Run: cinch self-update",
    }
}

pub struct RealDetector;

impl Detector for RealDetector {
    fn brew_prefix(&self) -> Option<PathBuf> {
        let out = std::process::Command::new("brew")
            .arg("--prefix")
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8(out.stdout).ok()?;
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    }

    fn dpkg_owner(&self, exe: &Path) -> Option<String> {
        let out = std::process::Command::new("dpkg")
            .arg("-S")
            .arg(exe)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8(out.stdout).ok()?;
        let pkg = s.split(':').next()?.trim();
        if pkg.is_empty() {
            None
        } else {
            Some(pkg.to_string())
        }
    }

    fn rpm_owner(&self, exe: &Path) -> Option<String> {
        let out = std::process::Command::new("rpm")
            .arg("-qf")
            .arg(exe)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8(out.stdout).ok()?;
        let pkg = s.trim();
        if pkg.is_empty() {
            None
        } else {
            Some(pkg.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeDetector {
        brew_prefix: Option<PathBuf>,
        dpkg: Option<String>,
        rpm: Option<String>,
    }
    impl Detector for FakeDetector {
        fn brew_prefix(&self) -> Option<PathBuf> {
            self.brew_prefix.clone()
        }
        fn dpkg_owner(&self, _: &Path) -> Option<String> {
            self.dpkg.clone()
        }
        fn rpm_owner(&self, _: &Path) -> Option<String> {
            self.rpm.clone()
        }
    }

    #[test]
    fn detects_homebrew_when_exe_under_prefix() {
        let d = FakeDetector {
            brew_prefix: Some(PathBuf::from("/opt/homebrew")),
            dpkg: None,
            rpm: None,
        };
        let exe = Path::new("/opt/homebrew/bin/cinch");
        assert_eq!(detect(exe, &d), InstallSource::Homebrew);
    }

    #[test]
    fn detects_apt_when_dpkg_owns_exe() {
        let d = FakeDetector {
            brew_prefix: None,
            dpkg: Some("cinch".to_string()),
            rpm: None,
        };
        let exe = Path::new("/usr/bin/cinch");
        assert_eq!(
            detect(exe, &d),
            InstallSource::Apt {
                pkg: "cinch".to_string()
            }
        );
    }

    #[test]
    fn detects_rpm_when_rpm_owns_exe() {
        let d = FakeDetector {
            brew_prefix: None,
            dpkg: None,
            rpm: Some("cinch-0.5.0-1.x86_64".to_string()),
        };
        let exe = Path::new("/usr/bin/cinch");
        assert_eq!(
            detect(exe, &d),
            InstallSource::Rpm {
                pkg: "cinch-0.5.0-1.x86_64".to_string()
            }
        );
    }

    #[test]
    fn falls_through_to_unknown_when_no_pm_owns_exe() {
        let d = FakeDetector {
            brew_prefix: None,
            dpkg: None,
            rpm: None,
        };
        let exe = Path::new("/usr/local/bin/cinch");
        assert_eq!(detect(exe, &d), InstallSource::Unknown);
    }

    #[test]
    fn ignores_non_cinch_dpkg_owner() {
        let d = FakeDetector {
            brew_prefix: None,
            dpkg: Some("coreutils".to_string()),
            rpm: None,
        };
        let exe = Path::new("/usr/bin/cinch");
        assert_eq!(detect(exe, &d), InstallSource::Unknown);
    }

    #[test]
    fn hint_strings_match_spec() {
        assert_eq!(hint(&InstallSource::Homebrew), "Run: brew upgrade cinch");
        assert_eq!(
            hint(&InstallSource::Apt {
                pkg: "cinch".to_string()
            }),
            "Run: sudo apt update && sudo apt install --only-upgrade cinch"
        );
        assert_eq!(
            hint(&InstallSource::Rpm {
                pkg: "cinch".to_string()
            }),
            "Run: sudo dnf upgrade cinch"
        );
        assert_eq!(hint(&InstallSource::Unknown), "Run: cinch self-update");
    }
}
