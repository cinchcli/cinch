//! Self-version nudge for the CLI.
//!
//! Compares the running binary's CARGO_PKG_VERSION against the latest
//! GitHub Release tag for `cinchcli/cinch`. Cached in
//! `dirs::cache_dir()/cinch/version-cache.json` with a 6h TTL. Reads are
//! synchronous; refreshes spawn a detached tokio task so the user's
//! command is never blocked. The nudge is suppressed when stderr is not
//! a TTY or `CINCH_NO_UPDATE_NUDGE` is set.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

const CACHE_TTL: Duration = Duration::from_secs(6 * 3600);
const GH_LATEST_CLI: &str = "https://api.github.com/repos/cinchcli/cinch/releases/latest";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VersionCache {
    pub cli_latest: Option<String>,
    pub cli_fetched_at: Option<SystemTime>,
}

pub fn cache_path() -> PathBuf {
    dirs::cache_dir()
        .map(|d| d.join("cinch").join("version-cache.json"))
        .unwrap_or_else(|| PathBuf::from(".cinch-version-cache.json"))
}

fn load_cache() -> VersionCache {
    std::fs::read_to_string(cache_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_cache(c: &VersionCache) {
    let path = cache_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(c) {
        let _ = std::fs::write(path, s);
    }
}

fn is_stale(at: Option<SystemTime>) -> bool {
    match at {
        None => true,
        Some(t) => SystemTime::now()
            .duration_since(t)
            .map(|d| d > CACHE_TTL)
            .unwrap_or(true),
    }
}

/// Returns `Some(latest_tag)` if the running binary is outdated AND it's
/// safe to nudge (stderr is a TTY and `CINCH_NO_UPDATE_NUDGE` is unset).
/// Returns `None` otherwise. Side effect: spawns a background refresh if
/// the cache is stale, so the next invocation has fresh data.
///
/// Must be called from within a tokio runtime (the spawn requires one).
pub fn check_self_outdated(own_version: &str) -> Option<String> {
    use is_terminal::IsTerminal;

    if std::env::var("CINCH_NO_UPDATE_NUDGE").is_ok() {
        return None;
    }
    if !std::io::stderr().is_terminal() {
        return None;
    }
    let cache = load_cache();
    if is_stale(cache.cli_fetched_at) {
        tokio::spawn(refresh());
    }
    let latest = cache.cli_latest?;
    let ours = semver::Version::parse(own_version).ok()?;
    let target = semver::Version::parse(latest.trim_start_matches('v')).ok()?;
    if ours < target {
        Some(latest)
    } else {
        None
    }
}

/// Returns the cached latest CLI tag without nudging. Used by
/// `cinch devices` to compute the per-device outdated marker.
pub fn cached_cli_latest() -> Option<String> {
    load_cache().cli_latest
}

/// Returns `true` if `reported` is a parseable semver strictly less than
/// `latest`. Returns `false` for any parse failure, missing input, or
/// equal/newer reported version. Used to gate the "(outdated)" marker
/// on the `cinch devices` table for CLI rows.
pub fn is_outdated(reported: &str, latest: &str) -> bool {
    let Ok(a) = semver::Version::parse(reported) else {
        return false;
    };
    let Ok(b) = semver::Version::parse(latest.trim_start_matches('v')) else {
        return false;
    };
    a < b
}

async fn refresh() {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(format!("cinch-cli/{}", env!("CARGO_PKG_VERSION")))
        .build()
    else {
        return;
    };
    let Ok(resp) = client.get(GH_LATEST_CLI).send().await else {
        return;
    };
    let Ok(body) = resp.json::<serde_json::Value>().await else {
        return;
    };
    let Some(tag) = body.get("tag_name").and_then(|v| v.as_str()) else {
        return;
    };
    let mut cache = load_cache();
    cache.cli_latest = Some(tag.to_string());
    cache.cli_fetched_at = Some(SystemTime::now());
    write_cache(&cache);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_stale_when_no_timestamp() {
        assert!(is_stale(None));
    }

    #[test]
    fn is_stale_when_older_than_ttl() {
        let old = SystemTime::now() - Duration::from_secs(7 * 3600);
        assert!(is_stale(Some(old)));
    }

    #[test]
    fn is_fresh_when_within_ttl() {
        let recent = SystemTime::now() - Duration::from_secs(60);
        assert!(!is_stale(Some(recent)));
    }

    #[test]
    fn is_outdated_true_when_reported_lower() {
        assert!(is_outdated("0.1.5", "v0.1.8"));
        assert!(is_outdated("0.1.5", "0.1.8")); // tolerate missing 'v' prefix
    }

    #[test]
    fn is_outdated_false_when_equal_or_newer() {
        assert!(!is_outdated("0.1.8", "v0.1.8"));
        assert!(!is_outdated("0.2.0", "v0.1.8"));
    }

    #[test]
    fn is_outdated_false_when_unparseable() {
        assert!(!is_outdated("dirty-build", "v0.1.8"));
        assert!(!is_outdated("0.1.5", "garbage"));
    }
}
