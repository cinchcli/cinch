#[cfg(test)]
mod tests {
    use crate::commands::clips::LocalClip;
    use crate::sync_status::{
        build_push_request, encrypt_or_drop_for_test as encrypt_or_drop, EncryptedPayload,
    };

    #[test]
    fn drops_clip_when_no_key() {
        assert!(encrypt_or_drop(None, b"plain").is_none());
    }

    #[test]
    fn encrypts_when_key_present() {
        let key = [9u8; 32];
        let result = encrypt_or_drop(Some(&key), b"plain").unwrap();
        assert!(result.encrypted);
        assert_ne!(result.body, "plain");
    }

    fn sample_clip() -> LocalClip {
        LocalClip {
            id: "01H".to_string(),
            user_id: "user".to_string(),
            content: "hello".to_string(),
            content_type: "text".to_string(),
            source: "remote:host".to_string(),
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: "label".to_string(),
            byte_size: 5,
            media_path: None,
            created_at: 0,
            synced: false,
            sync_state: "local".to_string(),
            is_pinned: false,
            pin_note: None,
            received_at: 0,
        }
    }

    #[test]
    fn push_request_carries_encrypted_payload_and_metadata() {
        let clip = sample_clip();
        let payload = EncryptedPayload {
            body: "ciphertext".to_string(),
            encrypted: true,
        };

        let req = build_push_request(&clip, payload);

        assert_eq!(req.content, "ciphertext");
        assert_eq!(req.content_type, "text");
        assert_eq!(req.source, "remote:host");
        assert_eq!(req.label, "label");
        assert_eq!(req.byte_size, 5);
        assert!(req.encrypted);
        assert_eq!(req.media_path, None);
    }

    #[test]
    fn push_request_serializes_to_relay_wire_shape() {
        let clip = sample_clip();
        let payload = EncryptedPayload {
            body: "ct".to_string(),
            encrypted: true,
        };

        let req = build_push_request(&clip, payload);
        let json = serde_json::to_string(&req).expect("serialize");

        // Sanity: required keys are present with the relay's snake_case names.
        assert!(json.contains(r#""content":"ct""#));
        assert!(json.contains(r#""content_type":"text""#));
        assert!(json.contains(r#""source":"remote:host""#));
        assert!(json.contains(r#""byte_size":5"#));
        assert!(json.contains(r#""encrypted":true"#));
        // Optional fields elided per proto omitempty rules.
        assert!(!json.contains("media_path"));
        assert!(!json.contains("target_device_id"));
    }
}
