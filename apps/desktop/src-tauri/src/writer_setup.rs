//! Shared `client_core::sync::Writer` setup pieces used by both the
//! initial-startup path (`startup::build_initial_writer_and_pusher`) and the
//! credential-swap restart path (`writer_restart::restart_writer`).
//!
//! These two call sites build the same `WsConfig` and the same WS-reconnect
//! catch-up callback. The only legitimate differences between them are *where*
//! the inputs come from (startup reads a `&Config` + fn params; restart reads
//! Tauri-managed state) and *how* the resulting `Writer`/`LocalPusher` are
//! stored. The construction below is provably identical, so it lives here once
//! and both sites pass their resolved values in.

use std::sync::Arc;

use crate::SharedStore;

/// Build the `WsConfig` for a `client_core::sync::Writer`.
///
/// Byte-identical to the struct literal both writer-start sites inlined. The
/// `media_fetcher` shares the REST client `Arc` (the Writer also owns
/// `rest_arc`) behind the `ClipTransport` seam, matching `ws::run`'s D-routing
/// media fetch for image clips that arrive with `media_path` set + empty
/// `content`.
pub(crate) fn build_ws_config(
    relay_url: String,
    token: String,
    enc_key: Option<[u8; 32]>,
    rest_arc: &Arc<client_core::http::RestClient>,
) -> client_core::ws::WsConfig {
    client_core::ws::WsConfig {
        relay_url,
        token,
        encryption_key: enc_key,
        client_info: Some(crate::app_state::build_client_info()),
        media_fetcher: Some(rest_arc.clone()),
    }
}

/// Build the WS reconnect catch-up callback.
///
/// Runs on initial connect AND every reconnect. `reconnect_catchup` drains
/// anything captured locally while offline (outbound flush) and pulls any clip
/// the relay broadcast while this device wasn't subscribed (inbound backfill) —
/// the relay does NOT replay missed events on resubscribe, so without backfill
/// here a `cinch push` landing during a WS hiccup stays invisible until next
/// launch. The closure also signals `devices_tx` so the consumer in lib.rs
/// `setup()` can emit `DevicesChanged` (other devices may have paired/revoked
/// while we were offline).
///
/// Returns `None` when there is no encryption key — without a key there is
/// nothing to decrypt on backfill, so the catch-up is skipped entirely. The
/// closure body is the same one both writer-start sites previously inlined;
/// only the source of `store`, `rest_arc`, `enc_key`, and `devices_tx` differs
/// between them.
pub(crate) fn build_reconnect_callback(
    store: SharedStore,
    rest_arc: Arc<client_core::http::RestClient>,
    enc_key: Option<[u8; 32]>,
    devices_tx: tokio::sync::mpsc::UnboundedSender<()>,
) -> Option<client_core::sync::OnConnectedCallback> {
    let key = enc_key?;
    let store_cb = store;
    let rest_cb = rest_arc;
    Some(Arc::new(move || {
        let store = store_cb.clone();
        let rest = rest_cb.clone();
        tauri::async_runtime::spawn(async move {
            let report = client_core::sync::reconnect_catchup(&store, &*rest, key).await;
            match &report.flush {
                Ok(r) if r.flushed > 0 || r.dropped > 0 => log::info!(
                    "desktop reconnect flush: flushed={} dropped={} remaining={}",
                    r.flushed,
                    r.dropped,
                    r.remaining,
                ),
                Ok(_) => {}
                Err(e) => log::debug!("desktop reconnect flush failed: {}", e),
            }
            match &report.backfill {
                Ok(n) if *n > 0 => {
                    log::info!("desktop reconnect backfill: inserted={}", n)
                }
                Ok(_) => {}
                Err(e) => log::debug!("desktop reconnect backfill failed: {}", e),
            }
        });
        // Signal the consumer in lib.rs setup() to emit DevicesChanged.
        // Other devices may have paired/revoked while we were offline.
        let _ = devices_tx.send(());
    }))
}
