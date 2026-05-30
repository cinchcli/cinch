//! Local capture path. Stores a clipboard clip in the shared store as
//! `sync_state = Local` and NEVER contacts the relay. Shared by the desktop
//! clipboard monitor and the CLI `cinch watch` daemon.

use crate::store::models::{StoredClip, SyncState};
use crate::store::{queries, Store, StoreError};
use ulid::Ulid;

/// Persist a captured clip as `Local`. Returns the generated clip id.
/// The relay is never contacted: capture builds local history only.
pub fn capture_local(
    store: &Store,
    source: &str,
    content_type_wire: &str,
    raw: Vec<u8>,
    byte_size: i64,
) -> Result<String, StoreError> {
    capture_local_with_metadata(
        store,
        source,
        None,
        None,
        None,
        content_type_wire,
        raw,
        byte_size,
    )
}

/// Persist a captured clip as `Local` with optional capture-source metadata.
/// The relay is never contacted: capture builds local history only.
pub fn capture_local_with_metadata(
    store: &Store,
    source: &str,
    source_app_id: Option<&str>,
    source_app: Option<&str>,
    source_url: Option<&str>,
    content_type_wire: &str,
    raw: Vec<u8>,
    byte_size: i64,
) -> Result<String, StoreError> {
    let clip_id = Ulid::new().to_string();
    let stored = StoredClip {
        id: clip_id.clone(),
        source: source.to_string(),
        source_key: None,
        source_app_id: source_app_id.map(str::to_string),
        source_app: source_app.map(str::to_string),
        source_url: source_url.map(str::to_string),
        content_type: content_type_wire.to_string(),
        content: Some(raw),
        media_path: None,
        byte_size,
        created_at: chrono::Utc::now().timestamp_millis(),
        pinned: false,
        pinned_at: None,
        sync_state: SyncState::Local,
    };
    queries::insert_clip(store, &stored)?;
    Ok(clip_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::RestClient;

    fn store() -> std::sync::Arc<Store> {
        std::sync::Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap())
    }

    #[test]
    fn capture_local_writes_local_state() {
        let s = store();
        let id = capture_local_with_metadata(
            &s,
            "remote:host",
            Some("com.apple.Safari"),
            Some("Safari"),
            Some("https://example.com/path"),
            "text",
            b"secret".to_vec(),
            6,
        )
        .unwrap();
        let row = queries::get_clip(&s, &id).unwrap().unwrap();
        assert_eq!(row.sync_state, SyncState::Local);
        assert_eq!(row.content.as_deref(), Some(&b"secret"[..]));
        assert_eq!(row.source_app_id.as_deref(), Some("com.apple.Safari"));
        assert_eq!(row.source_app.as_deref(), Some("Safari"));
        assert_eq!(row.source_url.as_deref(), Some("https://example.com/path"));
    }

    /// The core security invariant: a captured Local clip is never returned by
    /// the pending query and is never transmitted by a flush.
    #[tokio::test]
    async fn captured_clip_is_never_flushed() {
        let s = store();
        capture_local(&s, "remote:host", "text", b"secret".to_vec(), 6).unwrap();

        assert!(
            queries::list_pending_clips(&s).unwrap().is_empty(),
            "Local clip must not appear in the pending queue"
        );

        let client = RestClient::for_test_recording();
        let report = crate::sync::backlog_flusher::flush_once(&s, &client, [9u8; 32])
            .await
            .unwrap();
        assert_eq!(report.flushed, 0);
        assert!(
            client.recorded_pushes().is_empty(),
            "flush must make zero relay calls for a Local clip"
        );
        // The clip is untouched and still Local.
        let rows = queries::list_clips(&s, None, Some(10), None, false, 100).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sync_state, SyncState::Local);
    }
}
