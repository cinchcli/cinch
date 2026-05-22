use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::auth::{add_relay_profile, load_multi_config, transition, AuthState, AuthStateHandle};
use crate::commands::relays::{PendingAuthRelay, PendingRelayAdd};
use crate::protocol::MultiConfigHandle;
use crate::sync_status::{WsAbortHandle, WsStatus};

/// handle_deeplink — Tauri command for React to invoke when it receives a deep-link URL
/// via getCurrent() (cold-start case) or onOpenUrl (fallback for JS-side handling).
///
/// Parses the URL, extracts auth params, writes credentials, transitions state,
/// and spawns WS client.
#[tauri::command]
#[specta::specta]
pub async fn handle_deeplink(
    url: String,
    app: AppHandle,
    handle: State<'_, AuthStateHandle>,
    pending: State<'_, Arc<PendingRelayAdd>>,
    pending_auth: State<'_, Arc<PendingAuthRelay>>,
    mc: State<'_, MultiConfigHandle>,
    ws_abort: State<'_, Arc<WsAbortHandle>>,
) -> Result<(), String> {
    let parsed = url::Url::parse(&url).map_err(|e| format!("invalid URL: {}", e))?;

    // T-04-10: Validate this is an auth callback URL
    let is_auth = parsed.host_str() == Some("auth") || parsed.path() == "/auth/callback";
    if !is_auth {
        return Err("not an auth callback URL".into());
    }

    let token = parsed
        .query_pairs()
        .find(|(k, _)| k == "token")
        .map(|(_, v)| v.to_string())
        .ok_or("missing token param")?;
    let device_id = parsed
        .query_pairs()
        .find(|(k, _)| k == "device_id")
        .map(|(_, v)| v.to_string())
        .ok_or("missing device_id param")?;
    let user_id = parsed
        .query_pairs()
        .find(|(k, _)| k == "user_id")
        .map(|(_, v)| v.to_string())
        .ok_or("missing user_id param")?;
    let relay_url = parsed
        .query_pairs()
        .find(|(k, _)| k == "relay_url")
        .map(|(_, v)| v.to_string())
        .unwrap_or_else(|| "https://api.cinchcli.com".to_string());

    // T-04-09: validate token format (hex, 64 chars)
    if token.len() != 64 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("invalid token format".into());
    }

    // Validate relay_url scheme and host (prevents deep-link relay hijack)
    crate::validate_relay_url(&relay_url).map_err(|e| format!("invalid relay_url: {}", e))?;

    let hostname = client_core::machine::hostname_or_unknown();

    // Check if this is an "add new relay" flow or "update active relay" flow
    let pending_info = pending.take();
    let active_relay_id = if let Some(info) = pending_info {
        // Add new relay profile
        let relay_id = add_relay_profile(
            &user_id,
            &device_id,
            &token,
            &relay_url,
            &hostname,
            info.label.as_deref(),
            "",
        )
        .map_err(|e| format!("persist creds: {}", e))?;

        // Reload in-memory MultiConfig
        let new_mc = load_multi_config().map_err(|e| format!("load multi_config: {}", e))?;
        {
            let mut guard = mc.lock().unwrap();
            *guard = new_mc;
        }
        relay_id
    } else {
        // C1: Security — require a pending auth relay URL that matches the callback.
        // Peek first (don't consume) so a junk cold-start link cannot drain the state
        // before the legitimate callback is processed.
        let pending_relay_url = pending_auth.peek();
        if let Err(reason) = crate::validate_auth_callback(pending_relay_url.as_deref(), &relay_url)
        {
            log::warn!("handle_deeplink: rejected deep-link: {}", reason);
            return Ok(());
        }
        // Validation passed — consume the pending state so it cannot be replayed.
        pending_auth.clear();

        // Update active relay credentials atomically (original sign-in flow).
        client_core::auth_session::install_credentials(client_core::auth_session::InstallParams {
            user_id: &user_id,
            device_id: &device_id,
            token: &token,
            relay_url: &relay_url,
            hostname: &hostname,
            device_private_key: None,
            email: "",
            identity_provider: "",
            display_name: "",
        })
        .map_err(|e| format!("persist creds: {}", e))?;

        // Reload and get the active relay_id
        let new_mc = load_multi_config().map_err(|e| format!("load multi_config: {}", e))?;
        let relay_id = new_mc.active_relay_id.clone().unwrap_or_default();
        {
            let mut guard = mc.lock().unwrap();
            *guard = new_mc;
        }
        relay_id
    };

    transition(
        &app,
        &handle,
        AuthState::Authenticated {
            user_id: user_id.clone(),
            device_id: device_id.clone(),
            hostname: hostname.clone(),
            relay_url: relay_url.clone(),
            active_relay_id: active_relay_id.clone(),
            machine_id: client_core::machine::stable_machine_id(),
        },
    );

    // PRV-02: sync local retention preference to relay on sign-in
    {
        let relay = relay_url.clone();
        let tok = token.clone();
        let db_clone: Arc<crate::store::db::Database> = app
            .state::<Arc<crate::store::db::Database>>()
            .inner()
            .clone();
        tauri::async_runtime::spawn(async move {
            let remote_days = db_clone
                .get_setting("remote_retention_days")
                .ok()
                .flatten()
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(30);
            let url = format!("{}/devices/self/retention", relay.trim_end_matches('/'));
            let body = serde_json::json!({ "remote_retention_days": remote_days });
            let client = reqwest::Client::new();
            let _ = client
                .put(&url)
                .header("Authorization", format!("Bearer {}", tok))
                .json(&body)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await;
        });
    }

    // Joiner bootstrap runs concurrently so the WS connection is not delayed.
    {
        let bs_relay = relay_url.clone();
        let bs_token = token.clone();
        let bs_user = user_id.clone();
        let bs_device = device_id.clone();
        tokio::spawn(async move {
            crate::auth_bootstrap::run_joiner_flow(&bs_relay, &bs_token, &bs_user, &bs_device)
                .await;
        });
    }

    // Restart the client-core Writer with the new credentials.
    {
        let ws_status: State<'_, Arc<WsStatus>> = app.state();
        let relay_connected: State<'_, Arc<std::sync::atomic::AtomicBool>> = app.state();
        let rw_relay = relay_url.clone();
        let rw_token = token.clone();
        let rw_ws_status = ws_status.inner().clone();
        let rw_relay_connected = relay_connected.inner().clone();
        let app2 = app.clone();
        let jh = tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::restart_writer(
                &app2,
                &rw_relay,
                &rw_token,
                &rw_ws_status,
                &rw_relay_connected,
            )
            .await
            {
                log::error!("handle_deeplink: restart_writer failed: {}", e);
            }
        });
        ws_abort.replace(jh);
    }

    log::info!(
        "handle_deeplink auth complete: user={}, device={}, relay_id={}",
        user_id,
        device_id,
        active_relay_id,
    );
    Ok(())
}
