//! Local-clip backlog: enqueue offline-captured clips, flush them on
//! (re)connect in chronological order. See
//! docs/superpowers/specs/2026-05-20-clipboard-backlog-flush-design.md.

use chrono::{SecondsFormat, TimeZone, Utc};

/// Convert a Unix millis timestamp to the RFC3339 string the relay's parser
/// expects for `client_created_at`.
pub fn format_rfc3339_millis(unix_millis: i64) -> String {
    Utc.timestamp_millis_opt(unix_millis)
        .single()
        .expect("valid millis")
        .to_rfc3339_opts(SecondsFormat::Millis, true)
}

use crate::store::models::StoredClip;
use crate::store::{queries, Store, StoreError};
use ulid::Ulid;

/// Cap on number of `synced=false` rows allowed in the local store. Older
/// rows are dropped when this limit is exceeded.
pub const MAX_UNSYNCED: usize = 1000;

/// Persist a clip to the local store with a `local-<ULID>` id and
/// `sync_state = Pending`, then enforce the offline cap. This is the SEND
/// queue for offline explicit sends (NOT the capture path — see
/// `capture::capture_local` for `sync_state = Local` clipboard history).
/// Returns the temporary id — also used as the relay `idempotency_key` on
/// flush so the relay can dedup retries.
pub fn enqueue_local(
    store: &Store,
    source: &str,
    _label: &str,
    content_type: &str,
    raw: Vec<u8>,
    byte_size: i64,
) -> Result<String, StoreError> {
    let clip_id = format!("local-{}", Ulid::new());
    let stored = StoredClip {
        id: clip_id.clone(),
        source: source.to_string(),
        source_key: None,
        source_app_id: None,
        source_app: None,
        source_url: None,
        content_type: content_type.to_string(),
        content: Some(raw),
        media_path: None,
        byte_size,
        created_at: chrono::Utc::now().timestamp_millis(),
        pinned: false,
        pinned_at: None,
        sync_state: crate::store::models::SyncState::Pending,
    };
    queries::insert_clip(store, &stored)?;
    queries::enforce_offline_cap(store, MAX_UNSYNCED)?;
    Ok(clip_id)
}

use crate::crypto;
use crate::http::{HttpError, RestClient};
use crate::rest::PushRequest;

/// Push every `synced=false` clip in `created_at ASC` order. Stops on the
/// first transient (5xx / Network) error; drops rows on permanent (4xx /
/// Decode) errors; drops rows whose plaintext content is missing (media-only).
///
/// Idempotent via `idempotency_key = clip.id` — relay-side dedup absorbs
/// retries from interrupted flushes.
pub async fn flush_once(
    store: &crate::store::Store,
    client: &RestClient,
    enc_key: [u8; 32],
) -> Result<FlushReport, FlushError> {
    let pending = queries::list_pending_clips(store)?;
    let total = pending.len();
    let mut report = FlushReport::default();

    for clip in pending {
        let plaintext = match clip.content.clone() {
            Some(b) => b,
            None => {
                queries::delete_clip(store, &clip.id)?;
                report.dropped += 1;
                continue;
            }
        };

        let ciphertext = crypto::encrypt(&enc_key, &plaintext).map_err(FlushError::Crypto)?;
        let req = PushRequest {
            content: ciphertext,
            content_type: clip.content_type.clone(),
            label: String::new(),
            source: clip.source.clone(),
            media_path: None,
            byte_size: clip.byte_size,
            encrypted: true,
            client_created_at: Some(format_rfc3339_millis(clip.created_at)),
            idempotency_key: Some(clip.id.clone()),
        };

        match client.push_clip_json(&req).await {
            Ok(resp) => {
                queries::replace_id_and_mark_synced(store, &clip.id, &resp.clip_id)?;
                report.flushed += 1;
            }
            Err(e) if is_transient(&e) => {
                report.remaining = total - report.flushed - report.dropped;
                return Ok(report);
            }
            Err(e) => {
                log::warn!(
                    "backlog: dropping clip {} on permanent error: {}",
                    clip.id,
                    e
                );
                queries::delete_clip(store, &clip.id)?;
                report.dropped += 1;
            }
        }
    }

    Ok(report)
}

pub(crate) fn is_transient(e: &HttpError) -> bool {
    match e {
        HttpError::Network(_) => true,
        HttpError::Relay { status, .. } => (500..600).contains(status),
        HttpError::Unauthorized | HttpError::Decode(_) | HttpError::Build(_) => false,
    }
}

use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct FlushReport {
    pub flushed: usize,
    pub dropped: usize,
    pub remaining: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum FlushError {
    #[error("encryption failed: {0}")]
    Crypto(String),
    #[error("store error: {0}")]
    Store(#[from] crate::store::StoreError),
}

/// Reentrancy gate for `flush_once`. Process-local — call `try_enter` before
/// dispatching a flush; if it returns None, another flush is in progress and
/// the caller should skip silently.
pub struct FlushGate(AtomicBool);

impl FlushGate {
    pub const fn new() -> Self {
        Self(AtomicBool::new(false))
    }
    pub fn try_enter(&self) -> Option<FlushGuard<'_>> {
        if self.0.swap(true, Ordering::AcqRel) {
            None
        } else {
            Some(FlushGuard(&self.0))
        }
    }
}

impl Default for FlushGate {
    fn default() -> Self {
        Self::new()
    }
}

pub struct FlushGuard<'a>(&'a AtomicBool);

impl Drop for FlushGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::queries;
    use crate::store::Store;

    fn fresh_store() -> Store {
        Store::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn format_rfc3339_millis_emits_z_suffix_with_millis() {
        assert_eq!(
            format_rfc3339_millis(1_700_000_000_123),
            "2023-11-14T22:13:20.123Z"
        );
    }

    #[test]
    fn enqueue_local_persists_unsynced_with_local_prefix() {
        let store = fresh_store();
        let id = enqueue_local(&store, "remote:host", "label", "text", b"hello".to_vec(), 5)
            .expect("enqueue");
        assert!(
            id.starts_with("local-"),
            "id must start with 'local-' marker, got {id}"
        );
        let rows = queries::list_pending_clips(&store).unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.id, id);
        assert_eq!(row.sync_state, crate::store::models::SyncState::Pending);
        assert_eq!(row.source, "remote:host");
        assert_eq!(row.content.as_deref(), Some(&b"hello"[..]));
        assert_eq!(row.content_type, "text");
        assert_eq!(row.byte_size, 5);
    }

    #[test]
    fn enqueue_local_three_under_cap_all_survive() {
        let store = fresh_store();
        for _ in 0..3 {
            enqueue_local(&store, "s", "", "text", b"x".to_vec(), 1).unwrap();
        }
        let rows = queries::list_pending_clips(&store).unwrap();
        assert_eq!(
            rows.len(),
            3,
            "default cap is 1000; 3 enqueued must all survive"
        );
    }

    #[test]
    fn flush_gate_blocks_second_entrant_until_first_drops() {
        let gate = FlushGate::new();
        let g1 = gate.try_enter().unwrap();
        assert!(
            gate.try_enter().is_none(),
            "second try_enter must be blocked"
        );
        drop(g1);
        assert!(
            gate.try_enter().is_some(),
            "after guard drop, new entrant ok"
        );
    }

    fn seed_three_unsynced(store: &Store) {
        use crate::store::models::{StoredClip, SyncState};
        for (id, ts, content) in [
            ("local-a", 10i64, b"first" as &[u8]),
            ("local-b", 20, b"second"),
            ("local-c", 30, b"third"),
        ] {
            let c = StoredClip {
                id: id.into(),
                source: "s".into(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                content_type: "text".into(),
                content: Some(content.to_vec()),
                media_path: None,
                byte_size: content.len() as i64,
                created_at: ts,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Pending,
            };
            queries::insert_clip(store, &c).unwrap();
        }
    }

    #[tokio::test]
    async fn flush_once_pushes_in_chronological_order_and_swaps_ids() {
        let store = std::sync::Arc::new(fresh_store());
        seed_three_unsynced(&store);
        let key = [9u8; 32];
        let client = crate::http::RestClient::for_test_recording();
        let report = flush_once(&store, &client, key).await.expect("flush_once");
        assert_eq!(report.flushed, 3);
        assert_eq!(report.dropped, 0);
        assert_eq!(report.remaining, 0);

        // No more unsynced rows.
        let pending = queries::list_pending_clips(&store).unwrap();
        assert!(pending.is_empty(), "all rows should be synced");

        // The mock recorded calls in chronological order with idempotency keys.
        let calls = client.recorded_pushes();
        assert_eq!(calls.len(), 3);
        let keys: Vec<_> = calls.iter().map(|r| r.idempotency_key.clone()).collect();
        assert_eq!(
            keys,
            vec![
                Some("local-a".into()),
                Some("local-b".into()),
                Some("local-c".into())
            ]
        );

        // Each call carried client_created_at and was encrypted.
        for call in &calls {
            assert!(call.client_created_at.is_some());
            assert!(call.encrypted);
        }
    }

    #[tokio::test]
    async fn flush_once_stops_on_transient_and_reports_remaining() {
        let store = std::sync::Arc::new(fresh_store());
        seed_three_unsynced(&store);
        let key = [9u8; 32];
        let client = crate::http::RestClient::for_test_with_failures(vec![
            crate::http::FakePush::Ok,
            crate::http::FakePush::Network,
        ]);
        let report = flush_once(&store, &client, key).await.unwrap();
        assert_eq!(report.flushed, 1);
        assert_eq!(report.dropped, 0);
        assert_eq!(report.remaining, 2);
        assert_eq!(queries::list_pending_clips(&store).unwrap().len(), 2);
    }

    #[tokio::test]
    async fn flush_once_drops_on_permanent_and_continues() {
        let store = std::sync::Arc::new(fresh_store());
        seed_three_unsynced(&store);
        let key = [9u8; 32];
        let client = crate::http::RestClient::for_test_with_failures(vec![
            crate::http::FakePush::Ok,
            crate::http::FakePush::Relay {
                status: 413,
                msg: "payload too large".into(),
            },
            crate::http::FakePush::Ok,
        ]);
        let report = flush_once(&store, &client, key).await.unwrap();
        assert_eq!(report.flushed, 2);
        assert_eq!(report.dropped, 1);
        assert_eq!(queries::list_pending_clips(&store).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn flush_once_drops_rows_with_null_content() {
        use crate::store::models::{StoredClip, SyncState};
        let store = std::sync::Arc::new(fresh_store());
        let c = StoredClip {
            id: "local-nocontent".into(),
            source: "s".into(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            content_type: "text".into(),
            content: None,
            media_path: Some("/tmp/clip.png".into()),
            byte_size: 0,
            created_at: 10,
            pinned: false,
            pinned_at: None,
            sync_state: SyncState::Pending,
        };
        queries::insert_clip(&store, &c).unwrap();
        let key = [9u8; 32];
        let client = crate::http::RestClient::for_test_recording();
        let report = flush_once(&store, &client, key).await.unwrap();
        assert_eq!(report.flushed, 0);
        assert_eq!(report.dropped, 1);
        assert!(queries::list_pending_clips(&store).unwrap().is_empty());
        assert!(
            client.recorded_pushes().is_empty(),
            "no relay call for null-content row"
        );
    }
}
