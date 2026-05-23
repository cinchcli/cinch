use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_specta::Event;

use crate::auth::{transition, wipe_credentials, AuthState, AuthStateHandle};
use crate::commands::relays::PendingAuthRelay;

/// sign_out — calls POST /auth/device/revoke (best-effort), wipes credentials, transitions to LocalOnly.
/// Mirrors the CLI `auth logout` D-10 behavior.
#[tauri::command]
#[specta::specta]
pub async fn sign_out(
    app: AppHandle,
    handle: State<'_, AuthStateHandle>,
    pending_auth: State<'_, Arc<PendingAuthRelay>>,
    cache: State<'_, crate::commands::clips::DeviceCacheHandle>,
) -> Result<(), String> {
    let cfg = crate::protocol::Config::load().unwrap_or_default();
    if !cfg.token.is_empty() || !cfg.active_device_id.is_empty() {
        // Best-effort revoke — do not fail sign_out on network errors (D-10).
        let client = reqwest::Client::new();
        let revoke_body = serde_json::json!({ "device_id": cfg.active_device_id });
        let res = client
            .post(format!(
                "{}/auth/device/revoke",
                cfg.relay_url.trim_end_matches('/')
            ))
            .bearer_auth(&cfg.token)
            .json(&revoke_body)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await;
        if let Err(e) = res {
            log::warn!("sign_out: relay revoke failed (ignoring): {}", e);
        }
    }

    // I1: Belt-and-suspenders — clear any pending auth relay so a stale URL
    // cannot be exploited after the user has signed out.
    pending_auth.clear();

    wipe_credentials().map_err(|e| format!("wipe: {}", e))?;

    // Drop any device list cached under the old credentials so the next
    // list_devices does not serve relay-A data after the user has signed
    // out (or signed in to a different relay).
    cache.invalidate();
    if let Err(e) = crate::events::DevicesChanged.emit(&app) {
        log::warn!("DevicesChanged emit failed: {}", e);
    }

    transition(&app, &handle, AuthState::LocalOnly);
    Ok(())
}
