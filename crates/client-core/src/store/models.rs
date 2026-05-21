use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredClip {
    pub id: String, // ULID
    pub source: String,
    pub source_key: Option<String>,
    pub content_type: String,
    pub content: Option<Vec<u8>>,
    pub media_path: Option<String>,
    pub byte_size: i64,
    pub created_at: i64, // unix ms
    pub pinned: bool,
    pub pinned_at: Option<i64>,
    pub synced: bool,
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
