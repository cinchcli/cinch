//! Quick-edit a clip: produce a NEW local clip from edited text.
//!
//! Editing never mutates the original — it inserts a fresh local clip with a
//! new ULID, re-derived `content_type`, and `SyncState::Local`, inheriting the
//! original's provenance (source/app/url/label). This matches the clip model
//! (clips are point-in-time captures) and keeps the edited result local-only
//! until an explicit `send`.

use crate::store::models::{StoredClip, SyncState};
use crate::store::{queries, Store};
use std::error::Error;
use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum EditError {
    OriginalNotFound,
    Store(String),
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditError::OriginalNotFound => write!(f, "original clip not found"),
            EditError::Store(m) => write!(f, "local store error: {m}"),
        }
    }
}

impl Error for EditError {}

/// Insert a new local clip whose content is `new_text`, inheriting provenance
/// from `original_id`. Returns the newly inserted clip so callers don't have to
/// read it back. Does NOT touch the clipboard or the original clip.
pub fn apply_edit(
    store: &Store,
    original_id: &str,
    new_text: &str,
) -> Result<StoredClip, EditError> {
    let original = queries::get_clip(store, original_id)
        .map_err(|e| EditError::Store(e.to_string()))?
        .ok_or(EditError::OriginalNotFound)?;

    let content_type = crate::classify::detect(new_text.as_bytes())
        .as_wire()
        .to_string();

    let stored = StoredClip {
        id: ulid::Ulid::new().to_string(),
        source: original.source,
        source_key: original.source_key,
        source_app_id: original.source_app_id,
        source_app: original.source_app,
        source_url: original.source_url,
        label: original.label,
        content_type,
        content: Some(new_text.as_bytes().to_vec()),
        byte_size: new_text.len() as i64,
        created_at: chrono::Utc::now().timestamp_millis(),
        sync_state: SyncState::Local,
        ..Default::default()
    };

    queries::insert_clip(store, &stored).map_err(|e| EditError::Store(e.to_string()))?;
    Ok(stored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::models::StoredClip;
    use std::path::Path;

    fn store_with_clip(id: &str, content: &[u8], content_type: &str, source: &str) -> Store {
        let store = Store::open(Path::new(":memory:")).unwrap();
        queries::insert_clip(
            &store,
            &StoredClip {
                id: id.to_string(),
                source: source.to_string(),
                content_type: content_type.to_string(),
                content: Some(content.to_vec()),
                byte_size: content.len() as i64,
                created_at: 1,
                sync_state: SyncState::Local,
                label: Some("note".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        store
    }

    #[test]
    fn apply_edit_creates_new_local_clip_and_leaves_original() {
        let store = store_with_clip(
            "01HXAAAAAAAAAAAAAAAAAAAAAA",
            b"![[a.webp|703]]",
            "text",
            "remote:macbook",
        );
        let new_id = apply_edit(&store, "01HXAAAAAAAAAAAAAAAAAAAAAA", "![[a.webp]]")
            .unwrap()
            .id;

        assert_ne!(new_id, "01HXAAAAAAAAAAAAAAAAAAAAAA");
        let new_clip = queries::get_clip(&store, &new_id).unwrap().unwrap();
        assert_eq!(new_clip.content.as_deref(), Some(&b"![[a.webp]]"[..]));
        assert_eq!(new_clip.source, "remote:macbook");
        assert_eq!(new_clip.label.as_deref(), Some("note"));
        assert_eq!(new_clip.sync_state, SyncState::Local);
        assert!(!new_clip.pinned);

        let original = queries::get_clip(&store, "01HXAAAAAAAAAAAAAAAAAAAAAA")
            .unwrap()
            .unwrap();
        assert_eq!(original.content.as_deref(), Some(&b"![[a.webp|703]]"[..]));
    }

    #[test]
    fn apply_edit_reclassifies_content_type() {
        let store = store_with_clip("01HXBBBBBBBBBBBBBBBBBBBBBB", b"hello", "text", "local");
        let new_clip =
            apply_edit(&store, "01HXBBBBBBBBBBBBBBBBBBBBBB", "https://example.com").unwrap();
        assert_eq!(new_clip.content_type, "url");
    }

    #[test]
    fn apply_edit_errors_when_original_missing() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let err = apply_edit(&store, "01HXZZZZZZZZZZZZZZZZZZZZZZ", "x").unwrap_err();
        assert_eq!(err, EditError::OriginalNotFound);
    }
}
