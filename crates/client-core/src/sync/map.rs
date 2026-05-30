use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use crate::protocol::Clip;
use crate::store::models::StoredClip;

/// True when the clip's plaintext is binary (an image, or anything routed
/// through `media_path`). Must match `ws::decrypt_clip_content`'s predicate
/// for re-encoding plaintext to base64, so the two stay in sync.
fn is_binary(c: &Clip) -> bool {
    c.media_path.as_deref().filter(|p| !p.is_empty()).is_some()
        || c.content_type.starts_with("image")
}

/// Convert a wire [`Clip`] into a [`StoredClip`].
///
/// **The HTTP layer does not decrypt.** Callers must decrypt any
/// `clip.encrypted == true` clips before calling this function.  Passing
/// ciphertext here will store it verbatim and break FTS5 search and downstream
/// rendering.
///
/// Returns `Ok(None)` for rows that should be skipped without surfacing an
/// error (e.g. a clip with an empty ID).
pub fn clip_wire_to_stored(c: &Clip) -> Result<Option<StoredClip>, String> {
    if c.clip_id.is_empty() {
        return Ok(None);
    }

    // Wire `created_at` is RFC 3339; convert to unix milliseconds.
    let created_at = chrono::DateTime::parse_from_rfc3339(&c.created_at)
        .map_err(|e| format!("bad created_at {:?}: {e}", c.created_at))?
        .timestamp_millis();

    // Wire `content` is a plain UTF-8 String for text clips, or base64-encoded
    // bytes for binary clips (the proto `content` field is `string`, so the
    // decrypt path in `ws::decrypt_clip_content` re-encodes binary plaintext
    // to base64). Decode binary back to raw bytes so the local store is
    // type-agnostic and downstream readers (`apps/desktop/.../media.rs`) can
    // sniff magic bytes directly.
    let content: Option<Vec<u8>> = if c.content.is_empty() {
        None
    } else if is_binary(c) {
        Some(
            STANDARD
                .decode(c.content.as_bytes())
                .map_err(|e| format!("base64 decode failed for binary clip: {e}"))?,
        )
    } else {
        Some(c.content.as_bytes().to_vec())
    };

    Ok(Some(StoredClip {
        id: c.clip_id.clone(),
        source: c.source.clone(),
        // source_key is not present on the wire Clip; populated later if needed.
        source_key: None,
        source_app_id: None,
        source_app: None,
        source_url: None,
        content_type: c.content_type.clone(),
        content,
        media_path: c.media_path.clone(),
        byte_size: c.byte_size,
        created_at,
        pinned: c.is_pinned,
        // pinned_at is not present on the wire Clip.
        pinned_at: None,
        sync_state: crate::store::models::SyncState::Synced,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_HEADER: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    fn clip(content_type: &str, content: String) -> Clip {
        Clip {
            clip_id: "c1".into(),
            user_id: "u1".into(),
            content,
            content_type: content_type.into(),
            source: "remote:test".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            encrypted: false,
            ..Default::default()
        }
    }

    #[test]
    fn image_clip_stores_raw_bytes_not_base64() {
        let b64 = STANDARD.encode(PNG_HEADER);
        let stored = clip_wire_to_stored(&clip("image", b64)).unwrap().unwrap();
        assert_eq!(stored.content.as_deref(), Some(&PNG_HEADER[..]));
    }

    #[test]
    fn legacy_image_mime_treated_as_binary() {
        let b64 = STANDARD.encode(PNG_HEADER);
        let stored = clip_wire_to_stored(&clip("image/png", b64))
            .unwrap()
            .unwrap();
        assert_eq!(stored.content.as_deref(), Some(&PNG_HEADER[..]));
    }

    #[test]
    fn text_clip_stores_utf8_bytes() {
        let stored = clip_wire_to_stored(&clip("text", "hello".into()))
            .unwrap()
            .unwrap();
        assert_eq!(stored.content.as_deref(), Some(&b"hello"[..]));
    }

    #[test]
    fn empty_content_yields_none() {
        let stored = clip_wire_to_stored(&clip("image", String::new()))
            .unwrap()
            .unwrap();
        assert!(stored.content.is_none());
    }

    #[test]
    fn invalid_base64_for_binary_returns_err() {
        let bad = clip("image", "###not-base64###".into());
        assert!(clip_wire_to_stored(&bad).is_err());
    }

    #[test]
    fn empty_clip_id_returns_ok_none() {
        let mut c = clip("text", "hi".into());
        c.clip_id = String::new();
        assert!(matches!(clip_wire_to_stored(&c), Ok(None)));
    }

    #[test]
    fn media_path_marks_clip_as_binary() {
        // When `media_path` is set (Task D), binary handling kicks in even
        // for non-image content_types. Mirrors ws::decrypt_clip_content.
        let b64 = STANDARD.encode(PNG_HEADER);
        let mut c = clip("application/octet-stream", b64);
        c.media_path = Some("clips/c1.bin".into());
        let stored = clip_wire_to_stored(&c).unwrap().unwrap();
        assert_eq!(stored.content.as_deref(), Some(&PNG_HEADER[..]));
    }
}
