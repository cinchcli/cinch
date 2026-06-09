//! Detect how the current `cinch` binary was installed.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallSource {
    Homebrew { cask: bool },
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
            // Follow the symlink (/opt/homebrew/bin/cinch → real binary); if it
            // can't be resolved, classify the input path as-is.
            let resolved = std::fs::canonicalize(exe).unwrap_or_else(|_| exe.to_path_buf());
            return InstallSource::Homebrew {
                cask: path_looks_like_cask(&resolved),
            };
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
        InstallSource::Homebrew { .. } => "Run: brew upgrade cinchcli",
        InstallSource::Apt { .. } => {
            "Run: sudo apt update && sudo apt install --only-upgrade cinch"
        }
        InstallSource::Rpm { .. } => "Run: sudo dnf upgrade cinch",
        InstallSource::Unknown => "Run: cinch update",
    }
}

/// A cask install resolves into an `.app` bundle (under `Caskroom` or
/// `/Applications`); a formula install resolves into the `Cellar`. Pure string
/// check so it's unit-testable. We anchor on `.app/Contents/` (the bundle's
/// real layout) rather than a bare `.app/` so a stray `foo.app`-named dir or a
/// `webapp/` path can't false-positive a formula into a cask.
fn path_looks_like_cask(resolved: &Path) -> bool {
    let s = resolved.to_string_lossy();
    s.contains("/Caskroom/") || s.contains(".app/Contents/") || s.contains("/Applications/")
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
        // Use a Cellar path that does not exist on disk so canonicalize() falls
        // back to the raw path, which is formula-shaped (no .app / Caskroom).
        let exe = Path::new("/opt/homebrew/Cellar/cinchcli/0.0.0-test/bin/cinch");
        assert_eq!(detect(exe, &d), InstallSource::Homebrew { cask: false });
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
    fn path_looks_like_cask_true_for_app_bundle() {
        assert!(path_looks_like_cask(Path::new(
            "/opt/homebrew/Caskroom/cinchcli/0.7.1/Cinch.app/Contents/MacOS/Cinch"
        )));
        assert!(path_looks_like_cask(Path::new(
            "/Applications/Cinch.app/Contents/MacOS/Cinch"
        )));
    }

    #[test]
    fn path_looks_like_cask_false_for_cellar() {
        assert!(!path_looks_like_cask(Path::new(
            "/opt/homebrew/Cellar/cinchcli/0.7.1/bin/cinch"
        )));
    }

    #[test]
    fn detects_homebrew_formula_for_cellar_path() {
        let d = FakeDetector {
            brew_prefix: Some(PathBuf::from("/opt/homebrew")),
            dpkg: None,
            rpm: None,
        };
        // canonicalize() fails on this nonexistent path, so detect() falls back to
        // classifying the input path itself — which is a Cellar (formula) path.
        let exe = Path::new("/opt/homebrew/Cellar/cinchcli/0.7.1/bin/cinch");
        assert_eq!(detect(exe, &d), InstallSource::Homebrew { cask: false });
    }

    #[test]
    fn detects_homebrew_cask_for_app_path() {
        let d = FakeDetector {
            brew_prefix: Some(PathBuf::from("/opt/homebrew")),
            dpkg: None,
            rpm: None,
        };
        // Nonexistent path, so canonicalize() falls back to the raw path — which
        // is cask-shaped (Caskroom + .app/Contents) and classifies as a cask.
        let exe = Path::new("/opt/homebrew/Caskroom/cinchcli/0.7.1/Cinch.app/Contents/MacOS/Cinch");
        assert_eq!(detect(exe, &d), InstallSource::Homebrew { cask: true });
    }
}
