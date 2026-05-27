//! Local-clip ingest path.
//!
//! Captures clips detected on the local clipboard, encrypts them, pushes to
//! the relay, then write-throughs to the shared store using the relay-assigned
//! clip ID. Mirrors the `cinch push` flow so the desktop and CLI converge on a
//! single push pipeline.
//!
//! When no encryption key is available, or when the relay push returns a
//! transient error, the clip is queued locally via
//! `backlog_flusher::enqueue_local` and returned as `PushOutcome::Queued`.
//! The `flush_once` task retries those rows on the next (re)connect.

use std::sync::Arc;

use crate::crypto;
use crate::http::{HttpError, RestClient};
use crate::rest::{ContentType, PushRequest};
use crate::store::models::StoredClip;
use crate::store::{queries, Store, StoreError};

/// Result of `LocalPusher::push_text` / `push_image_png`. The string in both
/// variants is the clip id known to the local store. Callers that need to
/// surface offline state to the user can match on `Queued`.
#[derive(Debug, Clone)]
pub enum PushOutcome {
    /// Push to relay succeeded; carries the relay-assigned clip ID.
    Synced(String),
    /// Push deferred — clip persisted locally with a `local-<ULID>` id and
    /// `synced=false`. Will be retried on the next `flush_once` trigger.
    Queued(String),
}

/// Encrypt + push + local write-through for clips originating on this device.
///
/// One per active relay. Cheap to clone (`Arc` inside) so it can be shared by
/// the clipboard polling loop and any other producer (e.g., a manual paste
/// command).
#[derive(Clone)]
pub struct LocalPusher {
    store: Arc<Store>,
    client: Arc<RestClient>,
    enc_key: Option<[u8; 32]>,
}

#[derive(Debug, thiserror::Error)]
pub enum IngestError {
    /// No encryption key configured. Currently unreachable on the push path
    /// because `LocalPusher` queues to backlog instead — kept for downstream
    /// callers that may still match on it.
    #[error("no encryption key available — clip dropped (E2EE required)")]
    NoEncryptionKey,
    #[error("encryption failed: {0}")]
    Crypto(String),
    #[error("clip not found or has no sendable content: {0}")]
    NotFound(String),
    #[error("relay push failed: {0}")]
    Push(#[from] HttpError),
    #[error("local store write failed: {0}")]
    Store(#[from] StoreError),
}

impl LocalPusher {
    pub fn new(store: Arc<Store>, client: Arc<RestClient>, enc_key: Option<[u8; 32]>) -> Self {
        Self {
            store,
            client,
            enc_key,
        }
    }

    /// Encrypt + push a text clip, then write to the local store using the
    /// relay-assigned ID. Returns `PushOutcome::Synced(clip_id)` on success,
    /// or `PushOutcome::Queued(local_id)` when no encryption key is available
    /// or the relay push fails with a transient error.
    ///
    /// `content_type` is the canonical wire classification — one of
    /// `ContentType::Text`, `Url`, or `Code`. Callers typically derive it
    /// from `classify::detect(&raw)` (the raw bytes are accepted directly,
    /// so callers do not need to UTF-8-validate up front).
    pub async fn push_text(
        &self,
        raw: Vec<u8>,
        source: &str,
        label: &str,
        content_type: ContentType,
    ) -> Result<PushOutcome, IngestError> {
        let original_size = raw.len() as i64;

        if let Some(key) = self.enc_key {
            match self
                .try_push_text_online(&key, &raw, source, label, content_type, original_size)
                .await
            {
                Ok(clip_id) => return Ok(PushOutcome::Synced(clip_id)),
                Err(IngestError::Push(e)) if crate::sync::backlog_flusher::is_transient(&e) => {
                    // Transient relay error — fall through to enqueue as Pending.
                }
                Err(e) => return Err(e),
            }
        }

        let clip_id = crate::sync::backlog_flusher::enqueue_local(
            &self.store,
            source,
            label,
            content_type.as_wire(),
            raw,
            original_size,
        )?;
        Ok(PushOutcome::Queued(clip_id))
    }

    /// Encrypt + push a PNG image, then write to the local store using the
    /// relay-assigned ID. Returns `PushOutcome::Synced(clip_id)` on success,
    /// or `PushOutcome::Queued(local_id)` when no encryption key is available
    /// or the relay push fails with a transient error.
    pub async fn push_image_png(
        &self,
        raw_png: Vec<u8>,
        source: &str,
        label: &str,
    ) -> Result<PushOutcome, IngestError> {
        let original_size = raw_png.len() as i64;

        if let Some(key) = self.enc_key {
            match self
                .try_push_image_png_online(&key, &raw_png, source, label, original_size)
                .await
            {
                Ok(clip_id) => return Ok(PushOutcome::Synced(clip_id)),
                Err(IngestError::Push(e)) if crate::sync::backlog_flusher::is_transient(&e) => {
                    // Transient relay error — fall through to enqueue as Pending.
                }
                Err(e) => return Err(e),
            }
        }

        let clip_id = crate::sync::backlog_flusher::enqueue_local(
            &self.store,
            source,
            label,
            ContentType::Image.as_wire(),
            raw_png,
            original_size,
        )?;
        Ok(PushOutcome::Queued(clip_id))
    }

    /// Send an already-stored clip the user explicitly chose to send (by id).
    /// Marks it `Pending`, encrypts + pushes, and on success swaps to the
    /// relay id + `Synced`. On a transient error the clip stays `Pending`
    /// (the backlog flusher retries); on a permanent error it reverts to
    /// `Local` so it is never stuck retrying.
    ///
    /// Broadcasts the clip to all of the user's devices.
    pub async fn send_stored(&self, clip_id: &str) -> Result<PushOutcome, IngestError> {
        let clip = queries::get_clip(&self.store, clip_id)?
            .ok_or_else(|| IngestError::NotFound(clip_id.to_string()))?;
        let plaintext = clip
            .content
            .clone()
            .ok_or_else(|| IngestError::NotFound(format!("{clip_id} (no content)")))?;
        let key = self.enc_key.ok_or(IngestError::NoEncryptionKey)?;

        // Encrypt while the clip is still `Local`. Encryption failure is
        // deterministic (bad key / AEAD), so doing it before `mark_pending`
        // keeps a failed encrypt from stranding the clip in `Pending` where
        // the backlog flusher would re-fail it every cycle.
        let ciphertext = crypto::encrypt(&key, &plaintext).map_err(IngestError::Crypto)?;

        queries::mark_pending(&self.store, clip_id)?;
        let req = PushRequest {
            content: ciphertext,
            content_type: clip.content_type.clone(),
            label: String::new(),
            source: clip.source.clone(),
            media_path: None,
            byte_size: clip.byte_size,
            encrypted: true,
            client_created_at: Some(crate::sync::backlog_flusher::format_rfc3339_millis(
                clip.created_at,
            )),
            idempotency_key: Some(clip_id.to_string()),
        };

        match self.client.push_clip_json(&req).await {
            Ok(resp) => {
                queries::replace_id_and_mark_synced(&self.store, clip_id, &resp.clip_id)?;
                Ok(PushOutcome::Synced(resp.clip_id))
            }
            Err(e) if crate::sync::backlog_flusher::is_transient(&e) => {
                Ok(PushOutcome::Queued(clip_id.to_string()))
            }
            Err(e) => {
                queries::mark_local(&self.store, clip_id)?;
                Err(IngestError::Push(e))
            }
        }
    }

    async fn try_push_text_online(
        &self,
        key: &[u8; 32],
        raw: &[u8],
        source: &str,
        label: &str,
        content_type: ContentType,
        original_size: i64,
    ) -> Result<String, IngestError> {
        let ciphertext = crypto::encrypt(key, raw).map_err(IngestError::Crypto)?;
        let wire = content_type.as_wire();
        let req = PushRequest {
            content: ciphertext,
            content_type: wire.to_string(),
            label: label.to_string(),
            source: source.to_string(),
            media_path: None,
            byte_size: original_size,
            encrypted: true,
            client_created_at: None,
            idempotency_key: None,
        };
        let resp = self.client.push_clip_json(&req).await?;
        self.write_through(&resp.clip_id, source, wire, raw.to_vec(), original_size)?;
        Ok(resp.clip_id)
    }

    async fn try_push_image_png_online(
        &self,
        key: &[u8; 32],
        raw_png: &[u8],
        source: &str,
        label: &str,
        original_size: i64,
    ) -> Result<String, IngestError> {
        let ciphertext = crypto::encrypt(key, raw_png).map_err(IngestError::Crypto)?;
        let req = PushRequest {
            content: ciphertext,
            content_type: ContentType::Image.as_wire().into(),
            label: label.to_string(),
            source: source.to_string(),
            media_path: None,
            byte_size: original_size,
            encrypted: true,
            client_created_at: None,
            idempotency_key: None,
        };
        let resp = self.client.push_clip_json(&req).await?;
        self.write_through(
            &resp.clip_id,
            source,
            ContentType::Image.as_wire(),
            raw_png.to_vec(),
            original_size,
        )?;
        Ok(resp.clip_id)
    }

    fn write_through(
        &self,
        clip_id: &str,
        source: &str,
        content_type: &str,
        raw: Vec<u8>,
        byte_size: i64,
    ) -> Result<(), IngestError> {
        let stored = StoredClip {
            id: clip_id.to_string(),
            source: source.to_string(),
            source_key: None,
            content_type: content_type.to_string(),
            content: Some(raw),
            media_path: None,
            byte_size,
            created_at: chrono::Utc::now().timestamp_millis(),
            pinned: false,
            pinned_at: None,
            sync_state: crate::store::models::SyncState::Synced,
        };
        queries::insert_clip(&self.store, &stored)?;
        // Watermark is best-effort — failure here doesn't lose the clip.
        let _ = queries::set_watermark(&self.store, clip_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::queries;
    use std::sync::Arc;

    fn fresh_store() -> Arc<Store> {
        Arc::new(Store::open(std::path::Path::new(":memory:")).unwrap())
    }

    fn offline_client() -> Arc<RestClient> {
        Arc::new(RestClient::for_test_offline())
    }

    #[tokio::test]
    async fn push_text_queues_when_no_key() {
        let store = fresh_store();
        let pusher = LocalPusher::new(store.clone(), offline_client(), None);
        let outcome = pusher
            .push_text(
                b"hello".to_vec(),
                "remote:host",
                "",
                crate::rest::ContentType::Text,
            )
            .await
            .expect("push_text");
        match outcome {
            PushOutcome::Queued(id) => assert!(id.starts_with("local-")),
            PushOutcome::Synced(_) => panic!("expected Queued, got Synced"),
        }
        let rows = queries::list_pending_clips(&store).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].content.as_deref(), Some(&b"hello"[..]));
    }

    #[tokio::test]
    async fn push_text_queues_when_network_fails() {
        let store = fresh_store();
        let pusher = LocalPusher::new(store.clone(), offline_client(), Some([9u8; 32]));
        let outcome = pusher
            .push_text(
                b"hello".to_vec(),
                "remote:host",
                "",
                crate::rest::ContentType::Text,
            )
            .await
            .expect("push_text");
        assert!(matches!(outcome, PushOutcome::Queued(_)));
    }

    #[tokio::test]
    async fn push_text_errors_on_permanent_failure() {
        let store = fresh_store();
        // Unauthorized is a permanent error — must NOT be queued.
        let client = std::sync::Arc::new(RestClient::for_test_with_failures(vec![
            crate::http::FakePush::Relay {
                status: 401,
                msg: "unauthorized".into(),
            },
        ]));
        let pusher = LocalPusher::new(store.clone(), client, Some([9u8; 32]));
        let res = pusher
            .push_text(
                b"hi".to_vec(),
                "remote:host",
                "",
                crate::rest::ContentType::Text,
            )
            .await;
        assert!(
            matches!(res, Err(IngestError::Push(_))),
            "permanent error must surface, not queue"
        );
        assert!(
            queries::list_pending_clips(&store).unwrap().is_empty(),
            "permanent failure must not enqueue a Pending clip"
        );
    }

    #[tokio::test]
    async fn push_image_png_queues_when_no_key() {
        let store = fresh_store();
        let pusher = LocalPusher::new(store.clone(), offline_client(), None);
        let outcome = pusher
            .push_image_png(b"\x89PNG\r\n\x1a\n".to_vec(), "remote:host", "")
            .await
            .expect("push_image_png");
        match outcome {
            PushOutcome::Queued(id) => assert!(id.starts_with("local-")),
            PushOutcome::Synced(_) => panic!("expected Queued, got Synced"),
        }
        let rows = queries::list_pending_clips(&store).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].content_type, "image");
    }

    #[tokio::test]
    async fn push_image_png_queues_when_network_fails() {
        let store = fresh_store();
        let pusher = LocalPusher::new(store.clone(), offline_client(), Some([9u8; 32]));
        let outcome = pusher
            .push_image_png(b"\x89PNG\r\n\x1a\n".to_vec(), "remote:host", "")
            .await
            .expect("push_image_png");
        assert!(matches!(outcome, PushOutcome::Queued(_)));
    }

    #[tokio::test]
    async fn send_stored_syncs_a_captured_local_clip() {
        let store = fresh_store();
        let id =
            crate::sync::capture::capture_local(&store, "remote:host", "text", b"hi".to_vec(), 2)
                .unwrap();
        let client = std::sync::Arc::new(RestClient::for_test_recording());
        let pusher = LocalPusher::new(store.clone(), client.clone(), Some([9u8; 32]));

        let outcome = pusher.send_stored(&id).await.expect("send_stored");
        assert!(matches!(outcome, PushOutcome::Synced(_)));
        assert_eq!(client.recorded_pushes().len(), 1, "exactly one relay push");
        // The local id was swapped for the relay id; old id is gone.
        assert!(queries::get_clip(&store, &id).unwrap().is_none());
    }

    #[tokio::test]
    async fn send_stored_reverts_to_local_on_permanent_error() {
        use crate::store::models::SyncState;
        let store = fresh_store();
        let id =
            crate::sync::capture::capture_local(&store, "s", "text", b"hi".to_vec(), 2).unwrap();
        let client = std::sync::Arc::new(RestClient::for_test_with_failures(vec![
            crate::http::FakePush::Relay {
                status: 401,
                msg: "unauthorized".into(),
            },
        ]));
        let pusher = LocalPusher::new(store.clone(), client, Some([9u8; 32]));

        let res = pusher.send_stored(&id).await;
        assert!(matches!(res, Err(IngestError::Push(_))));
        assert_eq!(
            queries::get_clip(&store, &id).unwrap().unwrap().sync_state,
            SyncState::Local,
            "permanent error must revert the clip to Local, not leave it Pending"
        );
    }

    #[tokio::test]
    async fn send_stored_stays_pending_on_transient_error() {
        use crate::store::models::SyncState;
        let store = fresh_store();
        let id =
            crate::sync::capture::capture_local(&store, "s", "text", b"hi".to_vec(), 2).unwrap();
        let pusher = LocalPusher::new(store.clone(), offline_client(), Some([9u8; 32]));

        let outcome = pusher.send_stored(&id).await.expect("send_stored");
        assert!(matches!(outcome, PushOutcome::Queued(_)));
        assert_eq!(
            queries::get_clip(&store, &id).unwrap().unwrap().sync_state,
            SyncState::Pending,
            "transient error must leave the clip Pending for the flusher to retry"
        );
    }

    #[tokio::test]
    async fn send_stored_unknown_id_is_not_found() {
        let store = fresh_store();
        let pusher = LocalPusher::new(store.clone(), offline_client(), Some([9u8; 32]));
        let res = pusher.send_stored("does-not-exist").await;
        assert!(matches!(res, Err(IngestError::NotFound(_))));
    }

    #[tokio::test]
    async fn send_stored_broadcasts() {
        let store = fresh_store();
        let id =
            crate::sync::capture::capture_local(&store, "remote:host", "text", b"hi".to_vec(), 2)
                .unwrap();
        let client = std::sync::Arc::new(RestClient::for_test_recording());
        let pusher = LocalPusher::new(store.clone(), client.clone(), Some([9u8; 32]));

        let outcome = pusher.send_stored(&id).await.expect("send_stored");
        assert!(matches!(outcome, PushOutcome::Synced(_)));
        let pushes = client.recorded_pushes();
        assert_eq!(pushes.len(), 1);
    }
}
