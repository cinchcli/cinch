//! Release manifest fetcher + per-device outdated comparison.
//!
//! On launch and every 6 hours after, the desktop fetches the single
//! CDN-backed `version.json` manifest published by `cinchcli/cinch`:
//!
//!   `https://github.com/cinchcli/cinch/releases/latest/download/version.json`
//!
//! The manifest returns a bare semver (e.g. `{"version":"0.7.1",...}`).
//! Because the monorepo is single-version, both the CLI and desktop
//! share this version, so one fetch populates both `LatestVersions`
//! fields. The result is cached in `app_local_data_dir()/
//! version-cache.json`. The cached values drive the version badge
//! rendered next to each device in `DevicesPanel` and the "Update"
//! button on the user's own outdated desktop row.
//!
//! Network errors are silent: if a fetch fails, the cache stays
//! whatever it was, and the UI keeps showing the previous comparison.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::Manager;

const CACHE_TTL: Duration = Duration::from_secs(6 * 3600);
const VERSION_JSON: &str =
    "https://github.com/cinchcli/cinch/releases/latest/download/version.json";

/// Latest published release tag for each binary, plus the unix
/// timestamp at which the desktop last refreshed it. Frontend reads
/// this verbatim to decide whether to render the "Update" affordance.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Default)]
pub struct LatestVersions {
    pub cli: Option<String>,
    pub desktop: Option<String>,
    /// Unix seconds.
    pub fetched_at: Option<u64>,
}

/// The CLI's release manifest. We read only `version` (single monorepo
/// version drives both CLI and desktop).
#[derive(Debug, Deserialize)]
pub struct VersionManifest {
    pub version: String,
}

/// Outcome of comparing a single device's reported version against the
/// cached latest tag for that client type. Anything we cannot parse or
/// classify lands in `Unknown` — never `UpToDate` — so the badge stays
/// neutral until we have real evidence.
#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
pub enum VersionStatus {
    UpToDate,
    Outdated,
    Unknown,
}

/// Pure comparison: returns `Outdated` when `reported` parses as a
/// strictly-lower semver than the cached tag for the same client type.
/// Returns `Unknown` if anything is missing or unparseable.
pub fn compare(
    reported: Option<&str>,
    client_type: Option<&str>,
    latest: &LatestVersions,
) -> VersionStatus {
    let (Some(reported), Some(ct)) = (reported, client_type) else {
        return VersionStatus::Unknown;
    };
    let target = match ct {
        "cli" => latest.cli.as_deref(),
        "desktop" => latest.desktop.as_deref(),
        _ => return VersionStatus::Unknown,
    };
    let Some(target) = target else {
        return VersionStatus::Unknown;
    };
    // Shared semver comparison (client-core) — `None` means unparseable on
    // either side, which stays `Unknown` here.
    match client_core::version::compare_versions(reported, target) {
        Some(std::cmp::Ordering::Less) => VersionStatus::Outdated,
        Some(_) => VersionStatus::UpToDate,
        None => VersionStatus::Unknown,
    }
}

fn cache_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_local_data_dir()
        .ok()
        .map(|d| d.join("version-cache.json"))
}

pub fn load_cache(app: &tauri::AppHandle) -> LatestVersions {
    cache_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_cache(app: &tauri::AppHandle, v: &LatestVersions) {
    let Some(p) = cache_path(app) else { return };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(v) {
        let _ = std::fs::write(p, s);
    }
}

/// Returns true if the cached `fetched_at` is missing or older than the
/// 6-hour TTL. Public so the `get_latest_versions` command can decide
/// whether to kick off a background refresh.
pub fn is_stale(v: &LatestVersions) -> bool {
    let Some(at) = v.fetched_at else { return true };
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    now.saturating_sub(at) > CACHE_TTL.as_secs()
}

/// Fetches the single CDN-backed `version.json` manifest, populates
/// both `cli` and `desktop` from it (they share the same monorepo
/// version), updates `fetched_at`, and writes the result back. On
/// total failure (e.g. reqwest builder error) returns the existing
/// cache unchanged.
pub async fn fetch_and_cache(app: tauri::AppHandle) -> LatestVersions {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(format!("cinch-desktop/{}", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(c) => c,
        Err(_) => return load_cache(&app),
    };

    let latest = fetch_version(&client).await;

    let mut current = load_cache(&app);
    if let Some(v) = latest {
        // Single monorepo version: CLI and desktop share it.
        current.cli = Some(v.clone());
        current.desktop = Some(v);
    }
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    current.fetched_at = Some(now);
    write_cache(&app, &current);
    current
}

async fn fetch_version(client: &reqwest::Client) -> Option<String> {
    let resp = client.get(VERSION_JSON).send().await.ok()?;
    let manifest = resp.json::<VersionManifest>().await.ok()?;
    Some(manifest.version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latest(cli: &str, desktop: &str) -> LatestVersions {
        LatestVersions {
            cli: Some(cli.to_string()),
            desktop: Some(desktop.to_string()),
            fetched_at: None,
        }
    }

    #[test]
    fn cli_up_to_date() {
        assert_eq!(
            compare(Some("0.1.8"), Some("cli"), &latest("v0.1.8", "v0.1.7")),
            VersionStatus::UpToDate,
        );
    }

    #[test]
    fn cli_outdated() {
        assert_eq!(
            compare(Some("0.1.5"), Some("cli"), &latest("v0.1.8", "v0.1.7")),
            VersionStatus::Outdated,
        );
    }

    #[test]
    fn desktop_up_to_date() {
        assert_eq!(
            compare(Some("0.1.7"), Some("desktop"), &latest("v0.1.8", "v0.1.7")),
            VersionStatus::UpToDate,
        );
    }

    #[test]
    fn desktop_outdated() {
        assert_eq!(
            compare(Some("0.1.5"), Some("desktop"), &latest("v0.1.8", "v0.1.7")),
            VersionStatus::Outdated,
        );
    }

    #[test]
    fn newer_than_latest_is_up_to_date() {
        // Pre-release devs shouldn't see a phantom "outdated" badge.
        assert_eq!(
            compare(Some("0.2.0"), Some("cli"), &latest("v0.1.8", "v0.1.7")),
            VersionStatus::UpToDate,
        );
    }

    #[test]
    fn missing_reported_is_unknown() {
        assert_eq!(
            compare(None, Some("cli"), &latest("v0.1.8", "v0.1.7")),
            VersionStatus::Unknown,
        );
    }

    #[test]
    fn missing_client_type_is_unknown() {
        assert_eq!(
            compare(Some("0.1.5"), None, &latest("v0.1.8", "v0.1.7")),
            VersionStatus::Unknown,
        );
    }

    #[test]
    fn unknown_client_type_is_unknown() {
        assert_eq!(
            compare(Some("0.1.5"), Some("chrome"), &latest("v0.1.8", "v0.1.7")),
            VersionStatus::Unknown,
        );
    }

    #[test]
    fn missing_latest_for_type_is_unknown() {
        let l = LatestVersions {
            cli: None,
            desktop: Some("v0.1.7".to_string()),
            fetched_at: None,
        };
        assert_eq!(
            compare(Some("0.1.5"), Some("cli"), &l),
            VersionStatus::Unknown,
        );
    }

    #[test]
    fn unparseable_reported_is_unknown() {
        // "dev-build" is neither plain semver nor pre-release tagged
        // semver, so we refuse to compare.
        assert_eq!(
            compare(Some("dev-build"), Some("cli"), &latest("v0.1.8", "v0.1.7")),
            VersionStatus::Unknown,
        );
    }

    #[test]
    fn pre_release_reported_compares_normally() {
        // semver pre-release suffix sorts before the base version, so
        // a `-dirty` build that's a notch behind the latest tag still
        // surfaces as outdated rather than getting hidden under
        // Unknown. This matches the spec: a developer running a
        // pre-release of an old base version *is* outdated.
        assert_eq!(
            compare(
                Some("0.1.5-dirty+abc"),
                Some("cli"),
                &latest("v0.1.8", "v0.1.7")
            ),
            VersionStatus::Outdated,
        );
    }

    #[test]
    fn is_stale_when_no_timestamp() {
        assert!(is_stale(&LatestVersions::default()));
    }

    #[test]
    fn is_stale_when_older_than_ttl() {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let old = LatestVersions {
            cli: None,
            desktop: None,
            fetched_at: Some(now.saturating_sub(7 * 3600)),
        };
        assert!(is_stale(&old));
    }

    #[test]
    fn is_fresh_when_within_ttl() {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let recent = LatestVersions {
            cli: None,
            desktop: None,
            fetched_at: Some(now.saturating_sub(60)),
        };
        assert!(!is_stale(&recent));
    }

    #[test]
    fn parses_version_json() {
        let body = r#"{"version":"0.7.1","published_at":1715000000}"#;
        let m: VersionManifest = serde_json::from_str(body).unwrap();
        assert_eq!(m.version, "0.7.1");
    }

    #[test]
    fn clean_version_compares_without_prefix() {
        // Regression: the old GitHub-API path returned "release/0.7.1" which
        // compare_versions() could not parse. version.json gives a bare semver.
        let l = LatestVersions {
            cli: Some("0.7.1".into()),
            desktop: Some("0.7.1".into()),
            fetched_at: None,
        };
        assert_eq!(
            compare(Some("0.7.0"), Some("desktop"), &l),
            VersionStatus::Outdated
        );
        assert_eq!(
            compare(Some("0.7.1"), Some("desktop"), &l),
            VersionStatus::UpToDate
        );
    }
}
