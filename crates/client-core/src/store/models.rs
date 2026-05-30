use serde::{Deserialize, Serialize};

/// Per-clip relay-sync state. The store default for a captured clip is
/// `Local`: it never leaves the device until the user explicitly sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncState {
    /// Captured locally, no intent to send. Never picked up by the flusher.
    Local,
    /// Explicitly requested to send, not yet relay-confirmed. The only state
    /// the backlog flusher retries.
    Pending,
    /// Relay-confirmed: sent by us, or received from the relay.
    Synced,
}

impl SyncState {
    pub fn as_str(self) -> &'static str {
        match self {
            SyncState::Local => "local",
            SyncState::Pending => "pending",
            SyncState::Synced => "synced",
        }
    }

    /// Parse a stored string. Any unrecognized value maps to `Local` so a
    /// corrupt row can never be auto-sent.
    pub fn from_str_lossy(s: &str) -> Self {
        match s {
            "pending" => SyncState::Pending,
            "synced" => SyncState::Synced,
            _ => SyncState::Local,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredClip {
    pub id: String, // ULID
    pub source: String,
    pub source_key: Option<String>,
    pub source_app_id: Option<String>,
    pub source_app: Option<String>,
    pub source_url: Option<String>,
    pub content_type: String,
    pub content: Option<Vec<u8>>,
    pub media_path: Option<String>,
    pub byte_size: i64,
    pub created_at: i64, // unix ms
    pub pinned: bool,
    pub pinned_at: Option<i64>,
    pub sync_state: SyncState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredDevice {
    pub id: String,
    pub hostname: String,
    pub nickname: Option<String>,
    pub source_key: Option<String>,
    pub machine_id: Option<String>,
    pub public_key: Option<String>,
    pub paired_at: Option<i64>,
    pub last_push_at: Option<i64>,
    pub online: bool,
    pub refreshed_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRow {
    pub source: String,
    pub clip_count: i64,
    pub last_seen: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetentionPref {
    pub device_id: String,
    pub days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AlertPref {
    pub source: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MatchInfo {
    pub id: String,
    pub source: String,
    pub content_type: String,
    pub created_at: i64,
    pub preview: String, // first 40 chars of content or "[binary type · NkB]"
}

#[cfg(test)]
mod sync_state_tests {
    use super::SyncState;

    #[test]
    fn as_str_and_back_roundtrip() {
        for s in [SyncState::Local, SyncState::Pending, SyncState::Synced] {
            assert_eq!(SyncState::from_str_lossy(s.as_str()), s);
        }
    }

    #[test]
    fn unknown_text_is_local() {
        // Unknown / corrupt values must never be treated as sendable.
        assert_eq!(SyncState::from_str_lossy("garbage"), SyncState::Local);
    }

    #[test]
    fn serde_is_lowercase() {
        assert_eq!(
            serde_json::to_string(&SyncState::Local).unwrap(),
            "\"local\""
        );
        assert_eq!(
            serde_json::to_string(&SyncState::Pending).unwrap(),
            "\"pending\""
        );
        assert_eq!(
            serde_json::to_string(&SyncState::Synced).unwrap(),
            "\"synced\""
        );
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("prefix must be at least 4 characters")]
    TooShort,
    #[error("no match found")]
    NotFound,
    #[error("ambiguous prefix; {} candidates", .candidates.len())]
    Ambiguous { candidates: Vec<MatchInfo> },
    #[error("store error: {0}")]
    Store(#[from] super::StoreError),
}
