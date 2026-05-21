//! Fetch and parse `version.json` from GitHub Releases.

use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct VersionManifest {
    pub version: String,
    pub published_at: i64,
}

const MANIFEST_URL: &str =
    "https://github.com/cinchcli/cinch/releases/latest/download/version.json";

pub async fn fetch_latest() -> Result<VersionManifest, FetchError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(FetchError::Build)?;
    let resp = client
        .get(MANIFEST_URL)
        .send()
        .await
        .map_err(FetchError::Request)?
        .error_for_status()
        .map_err(FetchError::Status)?;
    let manifest: VersionManifest = resp.json().await.map_err(FetchError::Parse)?;
    Ok(manifest)
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("failed to build HTTP client: {0}")]
    Build(reqwest::Error),
    #[error("request failed: {0}")]
    Request(reqwest::Error),
    #[error("non-success status: {0}")]
    Status(reqwest::Error),
    #[error("malformed JSON: {0}")]
    Parse(reqwest::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_well_formed_manifest() {
        let json = r#"{"version":"0.6.0","published_at":1715000000}"#;
        let m: VersionManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.version, "0.6.0");
        assert_eq!(m.published_at, 1_715_000_000);
    }

    #[test]
    fn rejects_malformed_json() {
        let json = r#"{"version":}"#;
        let r: Result<VersionManifest, _> = serde_json::from_str(json);
        assert!(r.is_err());
    }

    #[test]
    fn rejects_missing_version_field() {
        let json = r#"{"published_at":1715000000}"#;
        let r: Result<VersionManifest, _> = serde_json::from_str(json);
        assert!(r.is_err());
    }
}
