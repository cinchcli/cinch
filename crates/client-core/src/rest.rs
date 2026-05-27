//! REST DTOs for the relay's legacy HTTP+JSON endpoints.
//!
//! Wire types are generated from `proto/cinch/v1/*.proto` at build time
//! (see `build.rs`); this module re-exports them under the names the CLI
//! and desktop already use. The generated types preserve snake_case JSON +
//! Go-style `omitempty` semantics via per-field `skip_serializing_if`
//! attribute injection.
//!
//! `ContentType` is a thin Rust-side enum kept for the CLI's auto-detection
//! pipeline. On the wire it round-trips through the proto's `string`
//! `content_type` field via `From<ContentType> for &'static str` so callers
//! can keep producing strongly typed values.

use serde::{Deserialize, Serialize};

pub use crate::proto::cinch::v1::{
    DeviceCodeCompleteRequest, DeviceCodeDenyRequest, DeviceCodePollResponse,
    DeviceCodeStartRequest as DeviceCodeRequest, DeviceCodeStartResponse as DeviceCodeResponse,
    ErrorResponse, GetMeRequest, GetMeResponse, KeyBundleGetResponse as KeyBundleResponse,
    KeyBundlePutRequest, Plan, PushClipRequest as PushRequest, PushClipResponse as PushResponse,
    RegisterDevicePublicKeyRequest, RevokeDeviceRequest as DeviceRevokeRequest, Usage,
};

/// Content classification — wire values are lowercase strings (`"text"`,
/// `"image"`, etc.) matching the Go side's `protocol.ContentType` constants
/// and the `string content_type` field on the proto messages.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ContentType {
    Text,
    Url,
    Code,
    Image,
}

impl ContentType {
    pub fn as_wire(self) -> &'static str {
        match self {
            ContentType::Text => "text",
            ContentType::Url => "url",
            ContentType::Code => "code",
            ContentType::Image => "image",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_request_serializes_minimal_fields() {
        let req = PushRequest {
            content: "hi".into(),
            content_type: String::new(),
            label: String::new(),
            source: "remote:host".into(),
            media_path: None,
            byte_size: 2,
            encrypted: false,
            client_created_at: None,
            idempotency_key: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""content":"hi""#));
        assert!(json.contains(r#""source":"remote:host""#));
        assert!(json.contains(r#""byte_size":2"#));
        assert!(!json.contains("content_type"));
        assert!(!json.contains("ttl"));
        assert!(!json.contains("encrypted"));
        assert!(!json.contains("target_device_id"));
    }

    #[test]
    fn push_request_serializes_encrypted() {
        let req = PushRequest {
            content: "ciphertext".into(),
            content_type: ContentType::Image.as_wire().into(),
            label: "logo".into(),
            source: "remote:host".into(),
            media_path: None,
            byte_size: 1234,
            encrypted: true,
            client_created_at: None,
            idempotency_key: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains(r#""content_type":"image""#));
        assert!(json.contains(r#""label":"logo""#));
        assert!(!json.contains("ttl"));
        assert!(json.contains(r#""encrypted":true"#));
        assert!(!json.contains("target_device_id"));
    }

    #[test]
    fn push_response_deserializes() {
        let json = r#"{"clip_id":"01HABC","byte_size":42}"#;
        let resp: PushResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.clip_id, "01HABC");
        assert_eq!(resp.byte_size, 42);
    }

    #[test]
    fn content_type_lowercase() {
        let s = serde_json::to_string(&ContentType::Text).unwrap();
        assert_eq!(s, r#""text""#);
        let s = serde_json::to_string(&ContentType::Image).unwrap();
        assert_eq!(s, r#""image""#);
    }
}
