//! Lightweight status types and offline-queue helpers that were previously part
//! of `ws.rs`.  Extracted here so callers can reference them without depending
//! on the old hand-rolled WebSocket client (which was deleted in Task 4.3).
//!
//! `WsStatus` — thin shared string that tracks the connection state for the
//!   legacy `get_ws_status` Tauri command and the tray icon.
//!
//! `WsAbortHandle` — holds the abort handle for the *old* WS task; kept so
//!   callers compiled against it do not need immediate surgery.  The Phase 4
//!   `WriterHandle` owns the real long-lived connection via `client_core`.
//!
//! `EncryptedPayload`, `encrypt_or_drop`, `build_push_request` — offline-queue
//!   helpers used by the clipboard monitor and the offline flush path.

#[cfg(test)]
use crate::store::models::LocalClip;

// ---------------------------------------------------------------------------
// WsStatus
// ---------------------------------------------------------------------------

pub struct WsStatus(pub std::sync::Arc<std::sync::Mutex<String>>);

impl WsStatus {
    pub fn new() -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(
            "connecting".to_string(),
        )))
    }
    pub fn set(&self, s: &str) {
        *self.0.lock().unwrap() = s.to_string();
    }
    pub fn get(&self) -> String {
        self.0.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// WsAbortHandle
// ---------------------------------------------------------------------------

/// Holds the abort handle for the active WebSocket task.
///
/// In Task 4.1 the primary connection was moved to `client_core::sync::Writer`.
/// This handle is retained only for the legacy `spawn_ws_client` callers that
/// were not yet migrated; Task 4.3 removes those callers and this type becomes
/// a no-op stub.  It can be deleted in Phase 5.
pub struct WsAbortHandle(pub std::sync::Mutex<Option<tauri::async_runtime::JoinHandle<()>>>);

impl WsAbortHandle {
    pub fn new() -> Self {
        Self(std::sync::Mutex::new(None))
    }

    pub fn replace(&self, handle: tauri::async_runtime::JoinHandle<()>) {
        let mut guard = self.0.lock().unwrap();
        if let Some(old) = guard.take() {
            old.abort();
        }
        *guard = Some(handle);
    }

    #[allow(dead_code)]
    pub fn abort(&self) {
        let mut guard = self.0.lock().unwrap();
        if let Some(h) = guard.take() {
            h.abort();
        }
    }
}

// ---------------------------------------------------------------------------
// Offline-queue helpers (test-only)
//
// Pure wire-contract helpers retained so the offline-queue path can be
// unit-tested without a relay or app handle.  Production code drives the
// same path through `client_core::sync::Writer`; if those helpers are wired
// back into the monitor, drop the `cfg(test)` gates.
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) struct EncryptedPayload {
    pub body: String,
    pub encrypted: bool,
}

/// Encrypt `plaintext` with `key`, returning `Some(EncryptedPayload)` on success
/// or `None` if no key is available (drops the clip from the queue).
#[cfg(test)]
pub(crate) fn encrypt_or_drop(
    key: Option<&[u8; 32]>,
    plaintext: &[u8],
) -> Option<EncryptedPayload> {
    let key = key?;
    crate::crypto::encrypt(key, plaintext)
        .ok()
        .map(|ct| EncryptedPayload {
            body: ct,
            encrypted: true,
        })
}

/// Build the typed `PushRequest` the relay expects from an offline-queued clip
/// plus its encrypted payload.  Pulled out as a pure function so the wire
/// contract can be unit-tested without spinning up a relay or app handle.
#[cfg(test)]
pub(crate) fn build_push_request(
    clip: &LocalClip,
    payload: EncryptedPayload,
) -> client_core::rest::PushRequest {
    client_core::rest::PushRequest {
        content: payload.body,
        content_type: clip.content_type.clone(),
        label: clip.label.clone(),
        source: clip.source.clone(),
        media_path: clip.media_path.clone(),
        byte_size: clip.byte_size,
        encrypted: payload.encrypted,
        client_created_at: None,
        idempotency_key: None,
    }
}

#[cfg(test)]
pub(crate) use encrypt_or_drop as encrypt_or_drop_for_test;
