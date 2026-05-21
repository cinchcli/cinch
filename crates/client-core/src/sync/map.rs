use crate::protocol::Clip;
use crate::store::models::StoredClip;

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

    // Wire `content` is a plain String (text clips) or base64-encoded bytes
    // (binary clips). Store it as raw bytes so the local store is type-agnostic.
    let content: Option<Vec<u8>> = if c.content.is_empty() {
        None
    } else {
        Some(c.content.as_bytes().to_vec())
    };

    Ok(Some(StoredClip {
        id: c.clip_id.clone(),
        source: c.source.clone(),
        // source_key is not present on the wire Clip; populated later if needed.
        source_key: None,
        content_type: c.content_type.clone(),
        content,
        media_path: c.media_path.clone(),
        byte_size: c.byte_size,
        created_at,
        pinned: c.is_pinned,
        // pinned_at is not present on the wire Clip.
        pinned_at: None,
        synced: true,
    }))
}
