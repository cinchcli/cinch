//! Wire protocol types shared across CLI, desktop, and the relay's WS frame.
//!
//! `Clip` and `DeviceInfo` are re-exported from the in-crate `proto` module,
//! generated from `proto/cinch/v1/*.proto`. `WSMessage` and the action
//! constants stay hand-written: the WebSocket envelope's "action + 8 optional
//! siblings" shape doesn't map cleanly onto a proto oneof, and migrating it
//! would change the WS wire format. That work is tracked separately.
//!
//! Action constants must match the Go relay verbatim (see `protocol/ws.go`
//! Action* constants). Wire field names must not change without coordinated
//! updates across all consumers.

use serde::{Deserialize, Serialize};

pub use crate::proto::cinch::v1::{Clip, Device as DeviceInfo};

// WebSocket action constants (must match Go relay exactly).
pub const ACTION_NEW_CLIP: &str = "new_clip";
pub const ACTION_CLIP_DELETED: &str = "clip_deleted";
pub const ACTION_PING: &str = "ping";
pub const ACTION_PONG: &str = "pong";
#[allow(dead_code)]
pub const ACTION_REVOKED: &str = "revoked";
#[allow(dead_code)]
pub const ACTION_TOKEN_ROTATED: &str = "token_rotated";
#[allow(dead_code)]
pub const ACTION_KEY_EXCHANGE_REQUESTED: &str = "key_exchange_requested";
#[allow(dead_code)]
pub const ACTION_CLIP_PINNED: &str = "clip_pinned";
#[allow(dead_code)]
pub const ACTION_DEVICE_CODE_PENDING: &str = "device_code_pending";
pub const ACTION_CLIENT_HELLO: &str = "client_hello";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WSMessage {
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<Clip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_key_fingerprint: Option<String>,
    // device_code_pending (relay → desktop) — push-approval notification
    // for a remote machine that just initiated DeviceCodeStart. The
    // existing `hostname` field above is reused to carry the requester's
    // hostname.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_region: Option<String>,
    // client_hello (client → relay) — sent immediately after WS auth so
    // the relay can record the client's self-reported version and OS.
    // See `version::ClientInfo::client_hello_message`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_hello: Option<ClientHelloPayload>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClientHelloPayload {
    pub version: String,
    // "type" is a Rust reserved keyword; serialized as "type" via serde rename.
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub os: String,
}

impl WSMessage {
    pub fn pong() -> Self {
        Self {
            action: ACTION_PONG.to_string(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_new_clip_message() {
        let json = r#"{
            "action": "new_clip",
            "clip": {
                "clip_id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "user_id": "user123",
                "content": "hello world",
                "content_type": "text",
                "source": "remote:prod-api",
                "label": "",
                "byte_size": 11,
                "created_at": "2026-04-14T12:00:00Z",
                "ttl": 0
            }
        }"#;
        let msg: WSMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.action, ACTION_NEW_CLIP);
        let clip = msg.clip.unwrap();
        assert_eq!(clip.clip_id, "01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert_eq!(clip.content, "hello world");
        assert_eq!(clip.source, "remote:prod-api");
    }

    #[test]
    fn test_parse_ping_message() {
        let json = r#"{"action":"ping"}"#;
        let msg: WSMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.action, ACTION_PING);
    }

    #[test]
    fn test_parse_clip_deleted_message() {
        let json = r#"{"action":"clip_deleted","clip":{"clip_id":"del123","user_id":"u1","content":"","content_type":"text","source":"local","created_at":"2026-04-14T12:00:00Z"}}"#;
        let msg: WSMessage = serde_json::from_str(json).unwrap();
        assert_eq!(msg.action, ACTION_CLIP_DELETED);
        assert_eq!(msg.clip.unwrap().clip_id, "del123");
    }

    #[test]
    fn test_serialize_pong() {
        let msg = WSMessage::pong();
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains(r#""action":"pong""#));
        assert!(!json.contains("clip"));
    }
}
