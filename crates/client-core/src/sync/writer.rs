//! Long-lived WS-driven sync writer.
//!
//! `Writer::start` acquires the advisory lockfile so only one writer runs per
//! machine, performs an initial REST backfill, then subscribes to the relay
//! WebSocket.  Incoming `new_clip` events are decrypted (when an encryption key
//! is available) and inserted into the local store.  On disconnect the loop
//! receives `WsStatus::Disconnected` and `ws::run` reconnects automatically
//! with exponential backoff.
//!
//! `Writer::shutdown` signals the background task to stop and awaits it.

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{mpsc, Notify};
use tokio::task::JoinHandle;
use tracing::warn;

use super::lockfile::{LockKind, Lockfile};
use super::{map, reader};
use crate::http::RestClient;
use crate::protocol::Clip;
use crate::store::{queries, Store};
use crate::ws::{self, DecryptFailReason, WsConfig, WsEvent, WsStatus};

/// Callback invoked after a remote `new_clip` event has been successfully
/// decrypted and inserted into the local store. The closure receives the
/// already-decrypted wire `Clip` so callers can extract metadata (id, source,
/// content_type) without touching the store.
///
/// Wrapped in `Arc` so the same callback can be shared across the writer's
/// background task and any future re-spawns. `Send + Sync` lets the closure
/// cross task boundaries.
pub type OnNewClipCallback = Arc<dyn Fn(&Clip) + Send + Sync>;

/// Callback invoked every time `WsStatus::Connected` is observed on the relay
/// WebSocket stream (i.e., on initial connection and on every reconnect).
///
/// The callback is executed on a separate `tokio::spawn` task so a slow
/// handler cannot stall the WS event loop.  It must therefore be
/// `Send + Sync + 'static`.
pub type OnConnectedCallback = Arc<dyn Fn() + Send + Sync>;

/// Long-lived WS-driven sync writer.  One per machine, coordinated by the
/// `~/.cinch/sync.lock` advisory lockfile.
pub struct Writer {
    handle: Option<JoinHandle<()>>,
    stop: Arc<Notify>,
    _lock: Lockfile, // released on Drop
    on_connected: Option<OnConnectedCallback>,
}

impl Writer {
    /// Try to start a writer.  Returns `Ok(None)` if another writer already
    /// holds the lock.
    ///
    /// The caller must supply a `WsConfig` (relay URL + bearer token +
    /// optional 32-byte AES key).  The same `RestClient` is used for the
    /// initial REST backfill.
    ///
    /// Use [`Writer::with_on_connected`] on the returned value to register a
    /// reconnect callback before the first `WsStatus::Connected` event fires.
    pub async fn start(
        store: Arc<Store>,
        client: Arc<RestClient>,
        ws_cfg: WsConfig,
        lock_path: PathBuf,
        kind: LockKind,
        on_new_clip: Option<OnNewClipCallback>,
        on_connected: Option<OnConnectedCallback>,
    ) -> std::io::Result<Option<Self>> {
        let lock = match Lockfile::try_acquire(&lock_path, kind)? {
            Some(l) => l,
            None => return Ok(None),
        };

        let stop = Arc::new(Notify::new());
        let stop_clone = stop.clone();
        let store_clone = store.clone();
        let client_clone = client.clone();
        let enc_key = ws_cfg.encryption_key;

        // Clone the callback Arc for the background task.  The Writer also
        // stores a copy so callers can inspect it, but the task holds the
        // authoritative reference used to fire on every Connected event.
        let on_connected_for_task = on_connected.clone();

        let handle = tokio::spawn(async move {
            // Initial REST backfill — bring the store up to date before
            // the WebSocket stream takes over.
            let _ = reader::backfill_once(
                &store_clone,
                &client_clone,
                reader::BackfillBudget::default(),
                enc_key.as_ref(),
            )
            .await;

            // Channel for WsEvent from the background ws::run task.
            let (tx, mut rx) = mpsc::channel::<WsEvent>(64);

            // ws::run already reconnects internally with exponential backoff,
            // so we just run it once and read from the channel until stop.
            let _ws_handle = tokio::spawn(ws::run(ws_cfg, tx));

            loop {
                tokio::select! {
                    _ = stop_clone.notified() => return,
                    maybe = rx.recv() => {
                        match maybe {
                            None => {
                                // Channel closed — ws::run exited (should not
                                // happen while the sender is alive).
                                return;
                            }
                            Some(event) => {
                                handle_ws_event(
                                    &store_clone,
                                    &client_clone,
                                    event,
                                    enc_key,
                                    on_new_clip.as_ref(),
                                    on_connected_for_task.as_ref(),
                                )
                                .await;
                            }
                        }
                    }
                }
            }
        });

        Ok(Some(Self {
            handle: Some(handle),
            stop,
            _lock: lock,
            on_connected,
        }))
    }

    /// Register (or replace) the callback that fires on every
    /// `WsStatus::Connected` event.
    ///
    /// This is a builder-style setter intended for use with a `Writer` that
    /// was constructed without a connected callback.  Note that if the writer
    /// background task has already started, changing this field has no effect
    /// on the running task — use the `on_connected` parameter of
    /// [`Writer::start`] to supply the callback at construction time.
    pub fn with_on_connected(mut self, cb: OnConnectedCallback) -> Self {
        self.on_connected = Some(cb);
        self
    }

    /// Signal the writer to stop and wait for it to finish.
    pub async fn shutdown(mut self) {
        self.stop.notify_waiters();
        if let Some(h) = self.handle.take() {
            let _ = h.await;
        }
    }
}

/// Fire the `on_connected` callback on a separate task so a slow handler
/// cannot stall the WS event loop.
///
/// Extracted as a free function so it can be unit-tested without a running
/// WebSocket connection.
pub(crate) fn dispatch_on_connected(cb: &OnConnectedCallback) {
    let cb = cb.clone();
    tokio::spawn(async move { cb() });
}

/// Process a single [`WsEvent`] from the relay subscription.
///
/// `NewClip` results in a store write. `KeyExchangeRequested` triggers
/// the ECDH bearer responder (when this device holds the encryption
/// key). Status changes are logged at debug level; decrypt failures
/// emit a warning. All other event kinds are no-ops pending Phase 5
/// handling.
async fn handle_ws_event(
    store: &Store,
    client: &RestClient,
    event: WsEvent,
    enc_key: Option<[u8; 32]>,
    on_new_clip: Option<&OnNewClipCallback>,
    on_connected: Option<&OnConnectedCallback>,
) {
    match event {
        WsEvent::NewClip { clip, plaintext: _ } => {
            // `ws::run` has already attempted decryption using the key supplied
            // in `WsConfig`.  After a successful decrypt `clip.encrypted` is
            // `false` and `clip.content` holds the decoded text (or base64 for
            // binary clips).  The `enc_key` was passed to `WsConfig` upstream so
            // decryption happens before the event reaches us here.
            let _ = enc_key; // consumed upstream by ws::decode_message via WsConfig
            let clip = *clip; // unbox
                              // After a successful decrypt, ws::run sets clip.encrypted = false
                              // and clip.content to the decoded string. If the clip is still
                              // marked encrypted here, the decrypt failed and we should not store
                              // ciphertext — the event type would be ClipDecryptFailed, not
                              // NewClip. So any NewClip with encrypted=false is safe to store.
            if clip.encrypted {
                // Should not happen: ws::run converts decrypt-failed clips to
                // WsEvent::ClipDecryptFailed. Guard here anyway.
                warn!(
                    clip_id = %clip.clip_id,
                    "writer: received NewClip with encrypted=true — skipping"
                );
                return;
            }
            // `clip_wire_to_stored` is the single boundary that converts the
            // wire `Clip.content` String into store bytes. For binary clips
            // (image/*, or anything with `media_path`) it base64-decodes back
            // to raw bytes; for text clips it stores UTF-8 bytes. The store
            // therefore always holds raw plaintext, which is what `media.rs`
            // (`cinch://media/...`) and FTS5 search expect.
            match map::clip_wire_to_stored(&clip) {
                Ok(Some(stored)) => {
                    let inserted = match queries::insert_clip(store, &stored) {
                        Ok(()) => true,
                        Err(e) => {
                            warn!(clip_id = %stored.id, error = %e, "writer: insert_clip failed");
                            false
                        }
                    };
                    if inserted {
                        if let Err(e) = queries::set_watermark(store, &stored.id) {
                            warn!(clip_id = %stored.id, error = %e, "writer: set_watermark failed");
                        }
                        if let Some(cb) = on_new_clip {
                            cb(&clip);
                        }
                    }
                }
                Ok(None) => {
                    // Empty clip_id — silently skip.
                }
                Err(e) => {
                    warn!(clip_id = %clip.clip_id, error = %e, "writer: map_wire_to_stored failed");
                }
            }
        }

        WsEvent::ClipDecryptFailed { clip_id, reason } => {
            let reason_str = match reason {
                DecryptFailReason::MissingKey => "no encryption key available".into(),
                DecryptFailReason::TagFailed(e) => format!("key mismatch: {e}"),
            };
            warn!(clip_id = %clip_id, reason = %reason_str, "writer: skipping encrypted clip — decrypt failed");
        }

        WsEvent::ClipDeleted { clip_id: _ } => {
            // TODO(phase 5): propagate deletion to local store.
        }

        WsEvent::Revoked { reason } => {
            warn!(
                reason = ?reason,
                "writer: device revoked by relay — writer will stop receiving events"
            );
        }

        WsEvent::TokenRotated {
            token: _,
            device_id: _,
        } => {
            // TODO(phase 5): persist the rotated token via credstore.
        }

        WsEvent::KeyExchangeRequested { device_id } => {
            log::info!(
                "writer: received KeyExchangeRequested device_id={:?}",
                device_id
            );
            let Some(did) = device_id else {
                log::info!("writer: key_exchange_requested missing device_id — skipping");
                return;
            };
            let Some(key) = enc_key else {
                log::info!(
                    "writer: cannot bear key — no encryption key on this device (target_device_id={})",
                    did
                );
                return;
            };
            log::info!("writer: bearing key for target_device_id={}", did);
            if let Err(e) = crate::key_exchange::handle_event(client, &did, &key).await {
                log::warn!(
                    "writer: key_exchange responder failed: target_device_id={}, error={}",
                    did,
                    e
                );
            } else {
                log::info!(
                    "writer: posted encrypted key bundle to peer target_device_id={}",
                    did
                );
            }
        }

        WsEvent::Status(WsStatus::Connected) => {
            tracing::debug!("writer: WS connected");
            if let Some(cb) = on_connected {
                dispatch_on_connected(cb);
            }
        }
        WsEvent::Status(WsStatus::Disconnected) => {
            tracing::debug!("writer: WS disconnected — ws::run will reconnect");
        }
        WsEvent::Status(WsStatus::Connecting) => {
            tracing::debug!("writer: WS connecting");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn make_test_store() -> Arc<crate::store::Store> {
        Arc::new(crate::store::Store::open(Path::new(":memory:")).expect("in-memory store"))
    }

    fn make_test_client() -> Arc<crate::http::RestClient> {
        Arc::new(crate::http::RestClient::for_test_offline())
    }

    #[tokio::test]
    async fn writer_invokes_on_connected_on_status_connected() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_c = counter.clone();
        let cb: OnConnectedCallback = Arc::new(move || {
            counter_c.fetch_add(1, Ordering::SeqCst);
        });

        let store = make_test_store();
        let client = make_test_client();

        // Fire Connected twice and Disconnected once in between.
        handle_ws_event(
            &store,
            &client,
            WsEvent::Status(WsStatus::Connected),
            None,
            None,
            Some(&cb),
        )
        .await;

        handle_ws_event(
            &store,
            &client,
            WsEvent::Status(WsStatus::Disconnected),
            None,
            None,
            Some(&cb),
        )
        .await;

        handle_ws_event(
            &store,
            &client,
            WsEvent::Status(WsStatus::Connected),
            None,
            None,
            Some(&cb),
        )
        .await;

        // The callback fires on a tokio::spawn — yield to let those tasks run.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "callback should fire exactly once per Connected event"
        );
    }

    #[tokio::test]
    async fn writer_skips_callback_when_none_set() {
        let store = make_test_store();
        let client = make_test_client();

        // Should not panic when no on_connected callback is registered.
        handle_ws_event(
            &store,
            &client,
            WsEvent::Status(WsStatus::Connected),
            None,
            None,
            None,
        )
        .await;
    }
}
