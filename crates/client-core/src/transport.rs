//! [`ClipTransport`] — the relay clip API behind a single object-safe trait.
//!
//! The sync engine (`local_pusher`, `backlog_flusher`, `reader`) and the
//! WebSocket media fetcher used to depend on the concrete [`crate::http::RestClient`].
//! That coupled the long-lived, frequently-tested data path to the HTTP/reqwest
//! implementation and to `RestClient`'s `#[cfg(test)]` fakes. This trait is the
//! seam: those consumers now hold `Arc<dyn ClipTransport>` / `&dyn ClipTransport`,
//! so a test can inject a [`MockTransport`] with zero network, and a future
//! non-HTTP transport (e.g. Connect-RPC) can drop in without touching the engine.
//!
//! # Error type
//! The trait reuses [`HttpError`] rather than introducing a parallel
//! `TransportError`. `HttpError` already exposes no `reqwest` types and owns the
//! single-source-of-truth [`HttpError::is_transient`] used for retry decisions —
//! duplicating it would just create skew.
//!
//! # Scope
//! [`ClipTransport`] covers the full clip REST surface (push / pull / list /
//! mutate). The one-shot CLI `pull` command and desktop pin/delete commands
//! still call the inherent [`RestClient`] methods directly — dynamic dispatch
//! buys them nothing and the trait remains available if they ever want it.
//!
//! The streaming half of relay I/O lives behind the companion
//! [`StreamTransport`] trait: [`WsStreamTransport`] wraps the real
//! [`crate::ws::run`] reconnect loop in production, and tests inject a
//! [`MockStreamTransport`] that emits canned [`WsEvent`]s with no socket — so
//! the sync [`Writer`](crate::sync::Writer) event loop is testable end-to-end.

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::http::{HttpError, ListClipsFilter};
use crate::protocol::Clip;
use crate::rest::{PushRequest, PushResponse};
use crate::version::ClientInfo;
use crate::ws::{WsConfig, WsEvent};

/// The set of relay clip operations the client performs. Implemented by
/// [`crate::http::RestClient`] (the real HTTP transport) and, in tests, by
/// [`MockTransport`].
///
/// `Send + Sync` so it can live inside `Arc<dyn ClipTransport>` and cross the
/// tokio task boundary in the sync engine. `Debug` so containers that hold one
/// (e.g. `WsConfig`) can keep deriving `Debug`.
#[async_trait]
pub trait ClipTransport: Send + Sync + std::fmt::Debug {
    /// `POST /clips` — JSON body (text and the encrypted-binary path).
    async fn push_clip_json(&self, req: &PushRequest) -> Result<PushResponse, HttpError>;

    /// `GET /clips/latest?source=...` — most recent clip matching `source`.
    async fn get_latest_clip(&self, source: &str) -> Result<Clip, HttpError>;

    /// `GET /clips/latest` — most recent clip across all devices.
    async fn get_latest_clip_any(&self) -> Result<Clip, HttpError>;

    /// `GET /clips/latest?exclude_source=...` — latest clip not from `exclude_source`.
    async fn get_latest_clip_excluding(&self, exclude_source: &str) -> Result<Clip, HttpError>;

    /// `GET /clips?clip_id=...&limit=1` — fetch one clip by id.
    async fn get_clip_by_id(&self, clip_id: &str) -> Result<Clip, HttpError>;

    /// `GET /clips/{id}/media` — raw bytes for image clips.
    async fn get_clip_media(&self, clip_id: &str) -> Result<Vec<u8>, HttpError>;

    /// `GET /clips?...` — list clips with the given filter, newest-first.
    async fn list_clips(&self, filter: ListClipsFilter) -> Result<Vec<Clip>, HttpError>;

    /// `GET /clips[?since=...][&limit=...]` — list clips newer than `since`.
    async fn list_clips_since(
        &self,
        since: Option<chrono::DateTime<chrono::Utc>>,
        limit: u32,
    ) -> Result<Vec<Clip>, HttpError>;

    /// `DELETE /clips/{id}` — remove a clip (404 treated as success).
    async fn delete_clip(&self, clip_id: &str) -> Result<(), HttpError>;

    /// `POST /clips/{id}/pin` — set or clear pin state.
    async fn set_clip_pin(
        &self,
        clip_id: &str,
        is_pinned: bool,
        pin_note: Option<&str>,
    ) -> Result<(), HttpError>;

    /// The `ClientInfo` (version + client type) this transport reports.
    fn client_info(&self) -> &ClientInfo;
}

// `RestClient` keeps its inherent methods (so the many concrete call sites in
// the CLI and desktop need no `use` change). This impl just forwards to them.
//
// Note on delegation: inside each body, `self.method(..)` resolves to the
// *inherent* `RestClient` method, never back into this trait — Rust method
// resolution always prefers an inherent method over a same-named trait method.
// So there is no recursion here.
#[async_trait]
impl ClipTransport for crate::http::RestClient {
    async fn push_clip_json(&self, req: &PushRequest) -> Result<PushResponse, HttpError> {
        self.push_clip_json(req).await
    }

    async fn get_latest_clip(&self, source: &str) -> Result<Clip, HttpError> {
        self.get_latest_clip(source).await
    }

    async fn get_latest_clip_any(&self) -> Result<Clip, HttpError> {
        self.get_latest_clip_any().await
    }

    async fn get_latest_clip_excluding(&self, exclude_source: &str) -> Result<Clip, HttpError> {
        self.get_latest_clip_excluding(exclude_source).await
    }

    async fn get_clip_by_id(&self, clip_id: &str) -> Result<Clip, HttpError> {
        self.get_clip_by_id(clip_id).await
    }

    async fn get_clip_media(&self, clip_id: &str) -> Result<Vec<u8>, HttpError> {
        self.get_clip_media(clip_id).await
    }

    async fn list_clips(&self, filter: ListClipsFilter) -> Result<Vec<Clip>, HttpError> {
        self.list_clips(filter).await
    }

    async fn list_clips_since(
        &self,
        since: Option<chrono::DateTime<chrono::Utc>>,
        limit: u32,
    ) -> Result<Vec<Clip>, HttpError> {
        self.list_clips_since(since, limit).await
    }

    async fn delete_clip(&self, clip_id: &str) -> Result<(), HttpError> {
        self.delete_clip(clip_id).await
    }

    async fn set_clip_pin(
        &self,
        clip_id: &str,
        is_pinned: bool,
        pin_note: Option<&str>,
    ) -> Result<(), HttpError> {
        self.set_clip_pin(clip_id, is_pinned, pin_note).await
    }

    fn client_info(&self) -> &ClientInfo {
        self.client_info()
    }
}

/// A network-free [`ClipTransport`] for tests. Records every `push_clip_json`
/// call and returns a synthetic clip id; all read/mutation methods that a test
/// has not opted into return a clear "not configured" error rather than
/// touching the network. Demonstrates that the sync engine no longer requires a
/// concrete `RestClient`.
#[cfg(test)]
#[derive(Debug)]
pub struct MockTransport {
    pushes: std::sync::Mutex<Vec<PushRequest>>,
    client_info: ClientInfo,
}

#[cfg(test)]
impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
impl MockTransport {
    pub fn new() -> Self {
        Self {
            pushes: std::sync::Mutex::new(Vec::new()),
            client_info: ClientInfo::for_test(),
        }
    }

    /// Every `PushRequest` this transport has received, in order.
    pub fn recorded_pushes(&self) -> Vec<PushRequest> {
        self.pushes.lock().unwrap().clone()
    }
}

#[cfg(test)]
fn not_configured(method: &str) -> HttpError {
    HttpError::Network(format!("MockTransport: {method} not configured"))
}

#[cfg(test)]
#[async_trait]
impl ClipTransport for MockTransport {
    async fn push_clip_json(&self, req: &PushRequest) -> Result<PushResponse, HttpError> {
        let mut pushes = self.pushes.lock().unwrap();
        pushes.push(req.clone());
        Ok(PushResponse {
            clip_id: format!("01HMOCK{:020}", pushes.len() - 1),
            byte_size: 0,
        })
    }

    async fn get_latest_clip(&self, _source: &str) -> Result<Clip, HttpError> {
        Err(not_configured("get_latest_clip"))
    }

    async fn get_latest_clip_any(&self) -> Result<Clip, HttpError> {
        Err(not_configured("get_latest_clip_any"))
    }

    async fn get_latest_clip_excluding(&self, _exclude_source: &str) -> Result<Clip, HttpError> {
        Err(not_configured("get_latest_clip_excluding"))
    }

    async fn get_clip_by_id(&self, _clip_id: &str) -> Result<Clip, HttpError> {
        Err(not_configured("get_clip_by_id"))
    }

    async fn get_clip_media(&self, _clip_id: &str) -> Result<Vec<u8>, HttpError> {
        Err(not_configured("get_clip_media"))
    }

    async fn list_clips(&self, _filter: ListClipsFilter) -> Result<Vec<Clip>, HttpError> {
        Err(not_configured("list_clips"))
    }

    async fn list_clips_since(
        &self,
        _since: Option<chrono::DateTime<chrono::Utc>>,
        _limit: u32,
    ) -> Result<Vec<Clip>, HttpError> {
        Err(not_configured("list_clips_since"))
    }

    async fn delete_clip(&self, _clip_id: &str) -> Result<(), HttpError> {
        Err(not_configured("delete_clip"))
    }

    async fn set_clip_pin(
        &self,
        _clip_id: &str,
        _is_pinned: bool,
        _pin_note: Option<&str>,
    ) -> Result<(), HttpError> {
        Err(not_configured("set_clip_pin"))
    }

    fn client_info(&self) -> &ClientInfo {
        &self.client_info
    }
}

/// The relay event stream behind an object-safe trait — the streaming
/// counterpart to [`ClipTransport`].
///
/// Where `ClipTransport` models request/response clip operations,
/// `StreamTransport` models the long-lived subscription that pushes
/// [`WsEvent`]s until the receiver is dropped. The sync
/// [`Writer`](crate::sync::Writer) holds an `Arc<dyn StreamTransport>` so its
/// event loop can run against [`WsStreamTransport`] in production or a fake in
/// tests, with no real WebSocket required.
#[async_trait]
pub trait StreamTransport: Send + Sync {
    /// Connect to the relay event stream and forward decoded [`WsEvent`]s to
    /// `tx`. Implementations reconnect internally with backoff and return only
    /// once `tx` is closed (the caller dropped the receiver).
    async fn run_stream(&self, cfg: WsConfig, tx: mpsc::Sender<WsEvent>);
}

/// Production [`StreamTransport`]: the relay WebSocket via [`crate::ws::run`].
///
/// A zero-sized newtype so it is free to hold in an `Arc`. All the real
/// connect / decrypt / reconnect logic stays in `ws::run`; this is just the seam.
#[derive(Debug, Clone, Copy, Default)]
pub struct WsStreamTransport;

#[async_trait]
impl StreamTransport for WsStreamTransport {
    async fn run_stream(&self, cfg: WsConfig, tx: mpsc::Sender<WsEvent>) {
        crate::ws::run(cfg, tx).await
    }
}

/// A socket-free [`StreamTransport`] for tests. Emits a fixed list of
/// [`WsEvent`]s (in order) on the first `run_stream`, then returns — at which
/// point `tx` drops and the consumer's loop observes the channel close. Lets a
/// test drive the [`Writer`](crate::sync::Writer) event loop end-to-end without
/// a relay.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct MockStreamTransport {
    events: std::sync::Mutex<Vec<WsEvent>>,
}

#[cfg(test)]
impl MockStreamTransport {
    pub fn with_events(events: Vec<WsEvent>) -> Self {
        Self {
            events: std::sync::Mutex::new(events),
        }
    }
}

#[cfg(test)]
#[async_trait]
impl StreamTransport for MockStreamTransport {
    async fn run_stream(&self, _cfg: WsConfig, tx: mpsc::Sender<WsEvent>) {
        let events = std::mem::take(&mut *self.events.lock().unwrap());
        for event in events {
            if tx.send(event).await.is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn mock_transport_records_pushes_through_dyn_dispatch() {
        // Exercise the trait object exactly as the sync engine holds it.
        let transport: Arc<dyn ClipTransport> = Arc::new(MockTransport::new());
        let req = PushRequest {
            content: "hello".into(),
            ..Default::default()
        };
        let resp = transport.push_clip_json(&req).await.expect("push");
        assert!(resp.clip_id.starts_with("01HMOCK"));

        // Downcast-free verification via the concrete handle.
        let mock = MockTransport::new();
        let _ = mock.push_clip_json(&req).await.unwrap();
        assert_eq!(mock.recorded_pushes().len(), 1);
        assert_eq!(mock.recorded_pushes()[0].content, "hello");
    }

    #[tokio::test]
    async fn unconfigured_methods_error_without_network() {
        let transport: Arc<dyn ClipTransport> = Arc::new(MockTransport::new());
        let err = transport
            .get_clip_media("c1")
            .await
            .expect_err("not configured");
        assert!(err.to_string().contains("get_clip_media"));
        // A "not configured" error is modelled as a transient network error, so
        // it never masquerades as a deterministic auth/4xx failure.
        assert!(err.is_transient());
    }

    #[tokio::test]
    async fn mock_stream_transport_emits_in_order_then_closes() {
        let stream: Arc<dyn StreamTransport> = Arc::new(MockStreamTransport::with_events(vec![
            WsEvent::ClipDeleted {
                clip_id: "a".into(),
            },
            WsEvent::ClipDeleted {
                clip_id: "b".into(),
            },
        ]));
        let (tx, mut rx) = mpsc::channel(8);
        stream
            .run_stream(
                WsConfig {
                    relay_url: String::new(),
                    token: String::new(),
                    encryption_key: None,
                    client_info: None,
                    media_fetcher: None,
                },
                tx,
            )
            .await;
        // run_stream returned, so tx is dropped; drain the buffered events.
        let mut ids = Vec::new();
        while let Some(WsEvent::ClipDeleted { clip_id }) = rx.recv().await {
            ids.push(clip_id);
        }
        assert_eq!(ids, vec!["a".to_string(), "b".to_string()]);
    }
}
