//! One-shot startup helpers called from `run()` before the Tauri builder
//! is constructed. Extracted from `lib.rs` to keep the entry orchestration
//! readable.

use std::sync::{Arc, Mutex};

use log::info;

use crate::app_state::{
    build_client_info, build_offline_pusher_handle, LocalPusherHandle, WriterHandle,
};
use crate::protocol;
use crate::SharedStore;

/// Build the initial `(WriterHandle, LocalPusherHandle)` pair at app launch.
///
/// We do this in the outer `run()` scope (before Tauri's setup hook) so the
/// writer is started exactly once at launch with the credentials that were
/// live at startup. The writer handle is moved into managed state so Tauri
/// keeps it alive for the full process lifetime.
///
/// The LocalPusher is built independently — it does not require the lock,
/// so a reader-mode desktop (lock held by CLI/another desktop) can still
/// push locally-detected clips. Both handles are swapped together by
/// `restart_writer` on credential changes.
pub(crate) fn build_initial_writer_and_pusher(
    config: &protocol::Config,
    is_configured: bool,
    shared_store: &SharedStore,
    clip_notif_tx: tokio::sync::mpsc::UnboundedSender<client_core::protocol::Clip>,
    devices_changed_tx: tokio::sync::mpsc::UnboundedSender<()>,
) -> (WriterHandle, LocalPusherHandle) {
    if !(is_configured && !config.token.is_empty() && !config.relay_url.is_empty()) {
        // Pre-login: construct a LocalPusher with no key so captures queue
        // locally. The RestClient is never invoked because LocalPusher
        // short-circuits to enqueue_local when enc_key is None.
        let pusher_handle = build_offline_pusher_handle(shared_store, config);
        return (Mutex::new(None), pusher_handle);
    }

    let enc_key = client_core::credstore::read_encryption_key(&config.user_id);
    let rest_client = match client_core::http::RestClient::new(
        config.relay_url.clone(),
        config.token.clone(),
        build_client_info(),
    ) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("cannot build RestClient for Writer (non-fatal): {}", e);
            // Even without a working RestClient we still want a
            // LocalPusher so captures queue locally (the pusher
            // short-circuits to enqueue_local when enc_key is None
            // and on any transient relay error otherwise).
            let pusher_handle = build_offline_pusher_handle(shared_store, config);
            return (Mutex::new(None), pusher_handle);
        }
    };

    let rest_arc = Arc::new(rest_client);
    // ws::run uses this REST client to GET /clips/{id}/media for media-routed
    // image clips (D-routing). Cloned because the Writer also owns rest_arc.
    let ws_cfg = crate::writer_setup::build_ws_config(
        config.relay_url.clone(),
        config.token.clone(),
        enc_key,
        &rest_arc,
    );
    let pusher =
        client_core::sync::LocalPusher::new(shared_store.clone(), rest_arc.clone(), enc_key);
    let store_for_writer = shared_store.clone();
    let lock_p = crate::paths::lock_path();

    let initial_cb_tx = clip_notif_tx.clone();
    let on_new_clip: client_core::sync::OnNewClipCallback = Arc::new(move |clip| {
        let _ = initial_cb_tx.send(clip.clone());
    });

    // Reconnect catch-up: runs on initial connect AND every reconnect.
    // Flush drains anything captured locally while offline (outbound),
    // backfill pulls any clip the relay broadcast while this device
    // wasn't subscribed (inbound) — the relay does NOT replay missed
    // events on resubscribe, so without backfill here a `cinch push`
    // landing during a WS hiccup stays invisible until next launch.
    let on_connected = crate::writer_setup::build_reconnect_callback(
        shared_store.clone(),
        rest_arc.clone(),
        enc_key,
        devices_changed_tx.clone(),
    );

    let writer = match tauri::async_runtime::block_on(client_core::sync::Writer::start(
        store_for_writer,
        rest_arc.clone(),
        ws_cfg,
        lock_p,
        client_core::sync::LockKind::Desktop,
        Some(on_new_clip),
        on_connected,
    )) {
        Ok(Some(w)) => {
            info!("client-core sync::Writer started");
            Mutex::new(Some(w))
        }
        Ok(None) => {
            log::warn!("sync::Writer: lock held by another process, skipping");
            Mutex::new(None)
        }
        Err(e) => {
            log::warn!("sync::Writer::start failed (non-fatal): {}", e);
            Mutex::new(None)
        }
    };

    // T1: backlog flush at boot. Idempotent and cheap when the
    // queue is empty, so it's safe to run unconditionally as
    // long as we have an encryption key. Drains anything left
    // behind by a previous offline session before the WS
    // (re)connect-driven flush gets a chance to fire.
    if let Some(key) = enc_key {
        let store_for_flush = shared_store.clone();
        let rest_for_flush = rest_arc.clone();
        tauri::async_runtime::spawn(async move {
            match client_core::sync::flush_once(&store_for_flush, &*rest_for_flush, key).await {
                Ok(report) => {
                    if report.flushed > 0 || report.dropped > 0 {
                        log::info!(
                            "desktop boot flush: flushed={} dropped={} remaining={}",
                            report.flushed,
                            report.dropped,
                            report.remaining,
                        );
                    }
                }
                Err(e) => log::debug!("desktop boot flush failed: {}", e),
            }
        });
    }

    (writer, Arc::new(Mutex::new(Some(pusher))))
}
