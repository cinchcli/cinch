//! Compatibility shim used by `cinch device list` to compute the per-device
//! "(outdated)" marker. The actual update-check cache lives in
//! [`crate::update::cache`]; this module only exposes the read-side query +
//! a strict-semver comparator.

/// Returns the cached latest CLI tag, or `None` if no update check has
/// populated the cache yet.
pub fn cached_cli_latest() -> Option<String> {
    let path = crate::update::cache::default_path()?;
    let cache = crate::update::cache::read(&path)?;
    Some(cache.latest_version)
}

/// Returns `true` if `reported` is a parseable semver strictly less than
/// `latest`. Returns `false` for any parse failure, missing input, or
/// equal/newer reported version. Used to gate the "(outdated)" marker on
/// the `cinch device list` table for CLI rows.
pub fn is_outdated(reported: &str, latest: &str) -> bool {
    let Ok(a) = semver::Version::parse(reported) else {
        return false;
    };
    let Ok(b) = semver::Version::parse(latest.trim_start_matches('v')) else {
        return false;
    };
    a < b
}

#[cfg(test)]
mod tests {
    use super::*;

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
