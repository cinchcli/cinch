//! `restart_writer`: shut down the active `client_core::sync::Writer`,
//! rebuild the `LocalPusher` with fresh credentials, then start a new
//! Writer that reconnects to the relay with the updated token.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::Manager;

use crate::app_state::{build_client_info, ClipNotifierTx, LocalPusherHandle, WriterHandle};
use crate::sync_status;
use crate::SharedStore;

/// Replace the active `client_core::sync::Writer` with a fresh one built from
/// new credentials. Called from the deep-link auth-callback handler (and from
/// `commands/auth.rs` / `commands/relays.rs`) after a fresh sign-in so the
/// writer reconnects to the relay with the updated token.
///
/// Shuts down the previous writer (releasing the lock) before starting the
/// new one. There is a brief window between the two where no writer holds the
/// lock; that is acceptable — a second desktop instance that swoops in will
/// simply become writer and the first will fall back to reader on next start.
pub(crate) async fn restart_writer(
    app: &tauri::AppHandle,
    relay_url: &str,
    token: &str,
    ws_status: &Arc<sync_status::WsStatus>,
    relay_connected: &Arc<AtomicBool>,
) -> Result<(), String> {
    // Resolve the user_id for the encryption key lookup.
    let user_id = {
        let mc = app.state::<crate::protocol::MultiConfigHandle>();
        let guard = mc.lock().unwrap();
        guard
            .active_profile()
            .map(|p| p.user_id.clone())
            .unwrap_or_default()
    };

    let enc_key = client_core::credstore::read_encryption_key(&user_id);
    let ws_cfg = client_core::ws::WsConfig {
        relay_url: relay_url.to_string(),
        token: token.to_string(),
        encryption_key: enc_key,
        client_info: Some(build_client_info()),
    };

    let rest = client_core::http::RestClient::new(
        relay_url.to_string(),
        token.to_string(),
        build_client_info(),
    )
    .map_err(|e| e.to_string())?;
    let rest_arc = Arc::new(rest);

    let store: SharedStore = app.state::<SharedStore>().inner().clone();
    let lock_path = client_core::store::lock_path()
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp/cinch.lock"));

    // Rebuild the LocalPusher with the new credentials so the next clipboard
    // capture pushes through the live token. Done before swapping the Writer
    // so a capture racing the swap still has a working pusher.
    {
        let pusher = client_core::sync::LocalPusher::new(store.clone(), rest_arc.clone(), enc_key);
        let handle = app.state::<LocalPusherHandle>();
        let mut guard = handle.lock().map_err(|e| e.to_string())?;
        *guard = Some(pusher);
    }

    // T3: backlog flush after credentials propagate. Drains anything the
    // pre-login (no-key) pusher queued locally, plus any leftover unsynced
    // rows from a previous offline session, as soon as a usable enc_key
    // becomes available.
    if let Some(key) = enc_key {
        let store_for_flush = store.clone();
        let rest_for_flush = rest_arc.clone();
        tauri::async_runtime::spawn(async move {
            match client_core::sync::flush_once(&store_for_flush, &rest_for_flush, key).await {
                Ok(report) => {
                    if report.flushed > 0 || report.dropped > 0 {
                        log::info!(
                            "desktop credential-propagate flush: flushed={} dropped={} remaining={}",
                            report.flushed,
                            report.dropped,
                            report.remaining,
                        );
                    }
                }
                Err(e) => log::debug!("desktop credential-propagate flush failed: {}", e),
            }
        });
    }

    // Shut down the old writer first so it releases the lockfile.
    // Take the Writer out while holding the lock, then drop the lock before
    // calling shutdown().await — std::sync::MutexGuard is not Send and must
    // not be held across an await point.
    let old_writer = {
        let writer_handle = app.state::<WriterHandle>();
        let mut guard = writer_handle.lock().map_err(|e| e.to_string())?;
        guard.take()
    };
    if let Some(w) = old_writer {
        w.shutdown().await;
    }

    ws_status.set("connecting");
    relay_connected.store(false, Ordering::Relaxed);

    // Forward NewClip notifications from the rebuilt Writer through the same
    // mpsc channel that the consumer task spawned in `.setup` is draining,
    // so per-source desktop alerts keep firing after a credential swap.
    let cb_tx = app.state::<ClipNotifierTx>().inner().0.clone();
    let on_new_clip: client_core::sync::OnNewClipCallback = Arc::new(move |clip| {
        let _ = cb_tx.send(clip.clone());
    });

    // T2: backlog flush on every WS (re)connect for the rebuilt Writer.
    let on_connected: Option<client_core::sync::OnConnectedCallback> = if let Some(key) = enc_key {
        let store_cb = store.clone();
        let rest_cb = rest_arc.clone();
        Some(Arc::new(move || {
            let store = store_cb.clone();
            let rest = rest_cb.clone();
            tauri::async_runtime::spawn(async move {
                match client_core::sync::flush_once(&store, &rest, key).await {
                    Ok(report) => {
                        if report.flushed > 0 || report.dropped > 0 {
                            log::info!(
                                "desktop reconnect flush: flushed={} dropped={} remaining={}",
                                report.flushed,
                                report.dropped,
                                report.remaining,
                            );
                        }
                    }
                    Err(e) => log::debug!("desktop reconnect flush failed: {}", e),
                }
            });
        }))
    } else {
        None
    };

    match client_core::sync::Writer::start(
        store,
        rest_arc,
        ws_cfg,
        lock_path,
        client_core::sync::LockKind::Desktop,
        Some(on_new_clip),
        on_connected,
    )
    .await
    .map_err(|e| e.to_string())?
    {
        Some(new_writer) => {
            let writer_handle = app.state::<WriterHandle>();
            let mut guard = writer_handle.lock().map_err(|e| e.to_string())?;
            *guard = Some(new_writer);
            ws_status.set("connected");
            relay_connected.store(true, Ordering::Relaxed);
            log::info!("restart_writer: new Writer started for relay={}", relay_url);
        }
        None => {
            log::warn!("restart_writer: lock held by another process — running as reader");
        }
    }

    Ok(())
}
