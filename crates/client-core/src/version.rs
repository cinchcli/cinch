//! Self-version reporting helpers shared by CLI and desktop.
//!
//! Both clients pass their own CARGO_PKG_VERSION (binary-crate level, not
//! cinchcli-core's version) into `ClientInfo` at startup. The resulting
//! struct is consumed by RestClient (HTTP headers) and WsClient (hello).
//!
//! This module also owns the single semver comparison used by the CLI's
//! "(outdated)" device marker and the desktop's update prompt.

use std::cmp::Ordering;

use reqwest::header::{HeaderName, HeaderValue};

use crate::protocol::{ClientHelloPayload, WSMessage};

/// Compare a `reported` version string against a `latest` release tag (a
/// leading `v` on `latest` is tolerated). Returns `None` if either side fails
/// to parse as semver. Single source of truth for version comparison: the
/// CLI's `cinch device list` "(outdated)" marker and the desktop's update
/// prompt both map this `Ordering` to their own state, so the parse rules and
/// `v`-prefix tolerance can never diverge.
pub fn compare_versions(reported: &str, latest: &str) -> Option<Ordering> {
    let a = semver::Version::parse(reported).ok()?;
    let b = semver::Version::parse(latest.trim_start_matches('v')).ok()?;
    Some(a.cmp(&b))
}

/// `true` when `reported` parses as a semver strictly older than `latest`.
/// Any parse failure yields `false` (treated as "not known to be outdated").
pub fn is_outdated(reported: &str, latest: &str) -> bool {
    compare_versions(reported, latest) == Some(Ordering::Less)
}

#[cfg(test)]
mod version_compare_tests {
    use super::*;

    #[test]
    fn outdated_when_reported_is_lower() {
        assert!(is_outdated("0.1.5", "v0.1.8"));
        assert!(is_outdated("0.1.5", "0.1.8")); // tolerate missing 'v' prefix
    }

    #[test]
    fn not_outdated_when_equal_or_newer() {
        assert!(!is_outdated("0.1.8", "v0.1.8"));
        assert!(!is_outdated("0.2.0", "v0.1.8"));
    }

    #[test]
    fn unparseable_is_none_and_not_outdated() {
        assert_eq!(compare_versions("not-a-version", "1.0.0"), None);
        assert_eq!(compare_versions("1.0.0", "garbage"), None);
        assert!(!is_outdated("not-a-version", "1.0.0"));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientType {
    Cli,
    Desktop,
}

impl ClientType {
    pub fn as_str(self) -> &'static str {
        match self {
            ClientType::Cli => "cli",
            ClientType::Desktop => "desktop",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub client_type: ClientType,
    pub version: String,
}

pub const HEADER_CLIENT_VERSION: &str = "x-cinch-client-version";
pub const HEADER_CLIENT_TYPE: &str = "x-cinch-client-type";

impl ClientInfo {
    pub fn http_headers(&self) -> [(HeaderName, HeaderValue); 2] {
        [
            (
                HeaderName::from_static(HEADER_CLIENT_VERSION),
                HeaderValue::from_str(&self.version).expect("ascii semver"),
            ),
            (
                HeaderName::from_static(HEADER_CLIENT_TYPE),
                HeaderValue::from_static(self.client_type.as_str()),
            ),
        ]
    }

    pub fn client_hello_message(&self) -> WSMessage {
        WSMessage {
            action: crate::protocol::ACTION_CLIENT_HELLO.to_string(),
            client_hello: Some(ClientHelloPayload {
                version: self.version.clone(),
                type_: self.client_type.as_str().to_string(),
                os: std::env::consts::OS.to_string(),
            }),
            ..Default::default()
        }
    }

    /// Test-only helper. Returns a `ClientInfo` suitable for wiring through
    /// integration and unit tests that exercise `RestClient::new` and
    /// `WsConfig` without caring about a real CLI / desktop version.
    pub fn for_test() -> Self {
        Self {
            client_type: ClientType::Cli,
            version: "0.0.0-test".to_string(),
        }
    }
}
