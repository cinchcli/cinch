//! Compose manifest + cache + source into the 24h stderr nudge.

use crate::update::cache::Cache;
use crate::update::manifest::VersionManifest;
use crate::update::source::InstallSource;

const CHECK_INTERVAL_SECS: i64 = 24 * 60 * 60;
const GRACE_WINDOW_SECS: i64 = 6 * 60 * 60;
const CI_ENV_VARS: &[&str] = &[
    "CI",
    "GITHUB_ACTIONS",
    "BUILDKITE",
    "JENKINS_URL",
    "TF_BUILD",
];

#[derive(Debug, PartialEq, Eq)]
pub enum NotifyAction {
    Print { from: String, to: String },
    Silent,
}

/// Pure decision function — given inputs, returns the action. Easy to test.
pub fn decide(
    current_version: &str,
    latest: &VersionManifest,
    source: &InstallSource,
    now_unix: i64,
) -> NotifyAction {
    let current = match semver::Version::parse(current_version) {
        Ok(v) => v,
        Err(_) => return NotifyAction::Silent,
    };
    let next = match semver::Version::parse(&latest.version) {
        Ok(v) => v,
        Err(_) => return NotifyAction::Silent,
    };
    if next <= current {
        return NotifyAction::Silent;
    }
    let age = now_unix - latest.published_at;
    let is_managed = !matches!(source, InstallSource::Unknown);
    if is_managed && age < GRACE_WINDOW_SECS {
        return NotifyAction::Silent;
    }
    NotifyAction::Print {
        from: current.to_string(),
        to: next.to_string(),
    }
}

pub fn is_due_for_check(cache: Option<&Cache>, now_unix: i64) -> bool {
    match cache {
        None => true,
        Some(c) => now_unix - c.last_check_unix >= CHECK_INTERVAL_SECS,
    }
}

pub fn is_ci_environment(read_env: impl Fn(&str) -> Option<String>) -> bool {
    CI_ENV_VARS.iter().any(|k| read_env(k).is_some())
}

pub fn is_opted_out(read_env: impl Fn(&str) -> Option<String>) -> bool {
    read_env("CINCH_NO_UPDATE_NOTIFIER").is_some()
}

/// Best-effort: fetch + decide + print + update cache. Swallows all errors.
pub async fn maybe_notify() {
    use std::io::IsTerminal;

    if !std::io::stderr().is_terminal() {
        return;
    }
    if is_opted_out(|k| std::env::var(k).ok()) {
        return;
    }
    if is_ci_environment(|k| std::env::var(k).ok()) {
        return;
    }

    let cache_path = match crate::update::cache::default_path() {
        Some(p) => p,
        None => return,
    };
    let cached = crate::update::cache::read(&cache_path);
    let now_unix = chrono::Utc::now().timestamp();
    if !is_due_for_check(cached.as_ref(), now_unix) {
        return;
    }

    let manifest = match crate::update::manifest::fetch_latest().await {
        Ok(m) => m,
        Err(_) => return,
    };

    let _ = crate::update::cache::write(
        &cache_path,
        &Cache {
            last_check_unix: now_unix,
            latest_version: manifest.version.clone(),
            latest_published_unix: manifest.published_at,
        },
    );

    let exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };
    let source = crate::update::source::detect(&exe, &crate::update::source::RealDetector);
    let action = decide(env!("CARGO_PKG_VERSION"), &manifest, &source, now_unix);

    if let NotifyAction::Print { from, to } = action {
        eprintln!("A new version of cinch is available: {} → {}", from, to);
        eprintln!("Run: cinch update");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::update::source::InstallSource;

    fn make_manifest(version: &str, published_at: i64) -> VersionManifest {
        VersionManifest {
            version: version.to_string(),
            published_at,
        }
    }

    #[test]
    fn prints_when_newer_version_available_and_source_is_unknown() {
        let m = make_manifest("0.6.0", 1_715_000_000);
        let action = decide("0.5.0", &m, &InstallSource::Unknown, 1_715_000_000);
        assert_eq!(
            action,
            NotifyAction::Print {
                from: "0.5.0".into(),
                to: "0.6.0".into(),
            }
        );
    }

    #[test]
    fn silent_when_current_is_same_as_latest() {
        let m = make_manifest("0.5.0", 1_715_000_000);
        let action = decide("0.5.0", &m, &InstallSource::Unknown, 1_715_000_000);
        assert_eq!(action, NotifyAction::Silent);
    }

    #[test]
    fn silent_when_current_is_newer_than_latest() {
        let m = make_manifest("0.5.0", 1_715_000_000);
        let action = decide("0.6.0", &m, &InstallSource::Unknown, 1_715_000_000);
        assert_eq!(action, NotifyAction::Silent);
    }

    #[test]
    fn grace_window_suppresses_recent_releases_for_brew() {
        let m = make_manifest("0.6.0", 1_715_000_000);
        let recent = 1_715_000_000 + 60 * 60; // 1h after publish
        let action = decide(
            "0.5.0",
            &m,
            &InstallSource::Homebrew { cask: false },
            recent,
        );
        assert_eq!(action, NotifyAction::Silent);
    }

    #[test]
    fn grace_window_does_not_suppress_for_unknown() {
        let m = make_manifest("0.6.0", 1_715_000_000);
        let recent = 1_715_000_000 + 60 * 60;
        let action = decide("0.5.0", &m, &InstallSource::Unknown, recent);
        match action {
            NotifyAction::Print { .. } => (),
            NotifyAction::Silent => panic!("expected Print for Unknown source"),
        }
    }

    #[test]
    fn grace_window_expires_after_six_hours_for_brew() {
        let m = make_manifest("0.6.0", 1_715_000_000);
        let after_grace = 1_715_000_000 + GRACE_WINDOW_SECS + 1;
        let action = decide(
            "0.5.0",
            &m,
            &InstallSource::Homebrew { cask: false },
            after_grace,
        );
        match action {
            NotifyAction::Print { .. } => (),
            NotifyAction::Silent => panic!("expected Print after grace window"),
        }
    }

    #[test]
    fn malformed_current_version_is_silent() {
        let m = make_manifest("0.6.0", 1_715_000_000);
        let action = decide("not-a-version", &m, &InstallSource::Unknown, 1_715_000_000);
        assert_eq!(action, NotifyAction::Silent);
    }

    #[test]
    fn malformed_latest_version_is_silent() {
        let m = make_manifest("garbage", 1_715_000_000);
        let action = decide("0.5.0", &m, &InstallSource::Unknown, 1_715_000_000);
        assert_eq!(action, NotifyAction::Silent);
    }

    #[test]
    fn is_due_when_cache_missing() {
        assert!(is_due_for_check(None, 1_715_000_000));
    }

    #[test]
    fn is_due_when_cache_old() {
        let cache = Cache {
            last_check_unix: 1_715_000_000,
            latest_version: "0.5.0".into(),
            latest_published_unix: 1_714_000_000,
        };
        assert!(is_due_for_check(
            Some(&cache),
            1_715_000_000 + CHECK_INTERVAL_SECS
        ));
    }

    #[test]
    fn is_not_due_when_cache_fresh() {
        let cache = Cache {
            last_check_unix: 1_715_000_000,
            latest_version: "0.5.0".into(),
            latest_published_unix: 1_714_000_000,
        };
        assert!(!is_due_for_check(
            Some(&cache),
            1_715_000_000 + CHECK_INTERVAL_SECS - 1
        ));
    }

    #[test]
    fn detects_each_ci_env() {
        for var in CI_ENV_VARS {
            let v = var.to_string();
            assert!(is_ci_environment(|k| if k == v {
                Some("1".into())
            } else {
                None
            }));
        }
    }

    #[test]
    fn no_ci_env_means_not_ci() {
        assert!(!is_ci_environment(|_| None));
    }

    #[test]
    fn opt_out_env_is_detected() {
        assert!(is_opted_out(|k| if k == "CINCH_NO_UPDATE_NOTIFIER" {
            Some("1".into())
        } else {
            None
        }));
    }

    #[test]
    fn no_opt_out_when_env_unset() {
        assert!(!is_opted_out(|_| None));
    }
}
