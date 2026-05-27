//! Relay connection management.
//! Exactly one relay is active at a time; pair_with_token replaces the existing one.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};

use tauri_specta::Event;

use crate::auth::{add_relay_profile, load_multi_config, transition, AuthState, AuthStateHandle};
use crate::protocol::MultiConfigHandle;
use crate::sync_status::{WsAbortHandle, WsStatus};

// ─── PendingAuthRelay ────────────────────────────────────────────────────────

/// Transient marker set by `sign_in` when the desktop opens the browser for a
/// standard auth flow.  The deep-link `else` branch requires this to be present
/// *and* to match the `relay_url` carried in the callback URL — rejects crafted
/// deep-links that arrive without a prior login being initiated.
pub struct PendingAuthRelay(pub std::sync::Mutex<Option<String>>);

impl PendingAuthRelay {
    pub fn new() -> Self {
        Self(std::sync::Mutex::new(None))
    }
    #[allow(dead_code)]
    pub fn take(&self) -> Option<String> {
        self.0.lock().unwrap().take()
    }
    /// Peek at the current value without consuming it.
    pub fn peek(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }
    pub fn set(&self, relay_url: String) {
        *self.0.lock().unwrap() = Some(relay_url);
    }
    /// Clear the pending state (used on failure/timeout/success paths).
    pub fn clear(&self) {
        *self.0.lock().unwrap() = None;
    }
}

// ─── PendingRelayAdd ─────────────────────────────────────────────────────────

/// Transient marker set by the browser-OAuth flow so `handle_deeplink` knows to
/// replace the relay profile instead of just refreshing the active one.
pub struct PendingRelayAdd(pub std::sync::Mutex<Option<PendingRelayInfo>>);

pub struct PendingRelayInfo {
    #[allow(dead_code)]
    pub relay_url: String,
    pub label: Option<String>,
}

impl PendingRelayAdd {
    pub fn new() -> Self {
        Self(std::sync::Mutex::new(None))
    }
    pub fn take(&self) -> Option<PendingRelayInfo> {
        self.0.lock().unwrap().take()
    }
    #[allow(dead_code)]
    pub fn set(&self, info: PendingRelayInfo) {
        *self.0.lock().unwrap() = Some(info);
    }
}

// ─── pair_with_token ─────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct PairWithTokenRequest {
    pub relay_url: String,
    pub pair_token: String,
    pub label: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Type)]
pub struct PairWithTokenResult {
    pub relay_id: String,
    pub user_id: String,
    pub device_id: String,
}

/// Pair with a relay using a master token obtained from `cinch auth login`.
/// Clears any existing relay and replaces it with the new one.
#[allow(
    clippy::too_many_arguments,
    reason = "Tauri injects command state via parameters"
)]
#[tauri::command]
#[specta::specta]
pub async fn pair_with_token(
    app: AppHandle,
    mc: State<'_, MultiConfigHandle>,
    ws_abort: State<'_, Arc<WsAbortHandle>>,
    ws_status: State<'_, Arc<WsStatus>>,
    auth_handle: State<'_, AuthStateHandle>,
    relay_connected: State<'_, Arc<std::sync::atomic::AtomicBool>>,
    cache: State<'_, crate::commands::clips::DeviceCacheHandle>,
    req: PairWithTokenRequest,
) -> Result<PairWithTokenResult, String> {
    let relay = req.relay_url.trim().trim_end_matches('/').to_string();
    if relay.is_empty() {
        return Err("relay_url required".into());
    }
    if req.pair_token.trim().is_empty() {
        return Err("pair_token required".into());
    }

    let hostname = client_core::machine::hostname_or_unknown();

    let (priv_b64, pub_b64) = crate::crypto::generate_ephemeral_keypair();

    let fingerprint = {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        use sha2::{Digest, Sha256};
        let raw = URL_SAFE_NO_PAD
            .decode(&pub_b64)
            .map_err(|e| format!("decode pub key: {}", e))?;
        let digest = Sha256::digest(&raw);
        digest[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };

    let body = serde_json::json!({
        "pair_token": req.pair_token.trim(),
        "hostname": hostname,
        "device_public_key": pub_b64,
        "device_key_fingerprint": fingerprint,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/auth/pair", relay))
        .json(&body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("request failed: {}", e))?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED
        || resp.status() == reqwest::StatusCode::BAD_REQUEST
    {
        return Err(
            "Pair token invalid or expired. Ask for a new one: cinch auth regenerate-pair-token"
                .into(),
        );
    }
    if !resp.status().is_success() {
        let status = resp.status();
        let body_text = resp.text().await.unwrap_or_default();
        return Err(format!("pairing failed ({}): {}", status, body_text));
    }

    #[derive(Deserialize)]
    struct PairResp {
        token: String,
        user_id: String,
        device_id: String,
    }
    let pair_resp: PairResp = resp
        .json()
        .await
        .map_err(|e| format!("parse response: {}", e))?;

    // Clear existing relay profiles — single-relay invariant.
    {
        let mut guard = mc.lock().unwrap();
        guard.relays.clear();
        guard.active_relay_id = None;
        guard.save().map_err(|e| format!("clear relays: {}", e))?;
    }

    let relay_id = add_relay_profile(
        &pair_resp.user_id,
        &pair_resp.device_id,
        &pair_resp.token,
        &relay,
        &hostname,
        req.label.as_deref(),
        &priv_b64,
    )
    .map_err(|e| format!("store credentials: {}", e))?;

    // Reload in-memory MultiConfig
    let new_mc = load_multi_config().map_err(|e| format!("load: {}", e))?;
    {
        let mut guard = mc.lock().unwrap();
        *guard = new_mc;
    }

    // Transition auth state and spawn WebSocket
    let (ws_relay_url, ws_token, next_state) = {
        let guard = mc.lock().unwrap();
        let profile = guard
            .active_profile()
            .ok_or("no active relay after pairing")?;
        let token = if profile.token.is_empty() {
            crate::auth::read_credentials(&profile.to_config()).unwrap_or_default()
        } else {
            profile.token.clone()
        };
        let next_state = AuthState::Authenticated {
            user_id: profile.user_id.clone(),
            device_id: profile.device_id.clone(),
            hostname: profile.hostname.clone(),
            relay_url: profile.relay_url.clone(),
            active_relay_id: profile.id.clone(),
            machine_id: profile.machine_id.clone(),
        };
        (profile.relay_url.clone(), token, next_state)
    };

    transition(&app, &auth_handle, next_state);

    // Joiner bootstrap runs concurrently so the WS connection is not delayed.
    {
        let bs_relay = ws_relay_url.clone();
        let bs_token = ws_token.clone();
        let bs_user = pair_resp.user_id.clone();
        let bs_device = pair_resp.device_id.clone();
        tokio::spawn(async move {
            crate::auth_bootstrap::run_joiner_flow(&bs_relay, &bs_token, &bs_user, &bs_device)
                .await;
        });
    }

    // Restart the client-core Writer with the new credentials.
    {
        let rw_relay = ws_relay_url.clone();
        let rw_token = ws_token.clone();
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
                log::error!("pair_with_token: restart_writer failed: {}", e);
            }
        });
        ws_abort.replace(jh);
    }

    // Invalidate any device list cached under the previous relay's credentials.
    // pair_with_token replaces the active relay, so the next list_devices must
    // re-fetch against the new identity, not serve stale rows from relay A.
    cache.invalidate();
    if let Err(e) = crate::events::DevicesChanged.emit(&app) {
        log::warn!("DevicesChanged emit failed: {}", e);
    }

    Ok(PairWithTokenResult {
        relay_id,
        user_id: pair_resp.user_id,
        device_id: pair_resp.device_id,
    })
}
