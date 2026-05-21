use serde::{Deserialize, Serialize};

use crate::protocol::Clip as ProtoClip;

// NOTE: Type derive removed in Task 4.2. The authoritative Specta-exported
// LocalClip is now crate::commands::clips::LocalClip. This struct stays for
// sync_status.rs and tests. monitor.rs has been migrated (it now emits the
// new commands::clips::LocalClip via clip_received_stub); ws.rs is the
// remaining production caller. Task 4.3 will delete this struct after
// ws.rs moves to the new type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalClip {
    pub id: String,
    pub user_id: String,
    pub content: String,
    pub content_type: String,
    pub source: String,
    pub label: String,
    pub byte_size: i64,
    pub media_path: Option<String>,
    pub created_at: i64, // unix timestamp
    pub synced: bool,
    pub is_pinned: bool,
    pub pin_note: Option<String>,
    pub received_at: i64, // unix timestamp: when this desktop received the clip
}

impl LocalClip {
    pub fn from_proto(clip: &ProtoClip, received_at: i64) -> Self {
        Self {
            id: clip.clip_id.clone(),
            user_id: clip.user_id.clone(),
            content: clip.content.clone(),
            content_type: clip.content_type.clone(),
            source: clip.source.clone(),
            label: clip.label.clone(),
            byte_size: clip.byte_size,
            media_path: clip.media_path.clone(),
            created_at: parse_timestamp(&clip.created_at),
            synced: true,
            is_pinned: clip.is_pinned,
            pin_note: clip.pin_note.clone(),
            received_at,
        }
    }
}

fn parse_timestamp(s: &str) -> i64 {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return dt.timestamp();
    }
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_proto_preserves_relay_type() {
        let proto = ProtoClip {
            clip_id: "test123".into(),
            user_id: "user1".into(),
            content: r#"{"error": "timeout"}"#.into(),
            content_type: "text".into(),
            source: "remote:prod".into(),
            label: "".into(),
            byte_size: 20,
            media_path: None,
            created_at: "2026-04-14T12:00:00Z".into(),
            encrypted: false,
            is_pinned: false,
            pin_note: None,
        };
        let local = LocalClip::from_proto(&proto, chrono::Utc::now().timestamp());
        assert_eq!(local.content_type, "text");
        assert!(local.created_at > 0);
    }
}
