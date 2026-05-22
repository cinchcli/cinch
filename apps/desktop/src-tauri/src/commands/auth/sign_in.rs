use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::auth::{transition, AuthState, AuthStateHandle};
use crate::commands::relays::PendingAuthRelay;
use crate::protocol::MultiConfigHandle;
use crate::sync_status::{WsAbortHandle, WsStatus};

/// sign_in — browser-based sign-in using device-code polling.
///
/// Flow (deep-link-independent):
///   1. Transitions to Authenticating{SigningIn}
///   2. POSTs to {relay_url}/auth/device-code to get a device_code + verification_uri
///   3. Opens verification_uri in the system browser (includes device_code so OAuth
///      providers show the right page and the relay can complete the flow)
///   4. Returns immediately — a background tokio task polls
///      GET {relay_url}/auth/device-code/poll?code={device_code} every 3 seconds
///   5. When status == "complete", writes credentials and transitions to Authenticated
///
/// The existing cinch://auth/callback deep-link handler in lib.rs still fires as a
/// secondary path (legacy self-host servers that skip the device-code completion step).
#[tauri::command]
#[specta::specta]
pub fn sign_in(
    app: AppHandle,
    handle: State<'_, AuthStateHandle>,
    relay_url: String,
    provider: Option<String>,
) -> Result<(), String> {
    let auth_handle = handle.inner().clone();
    let relay = relay_url.trim().trim_end_matches('/').to_string();
    if relay.is_empty() {
        return Err("relay_url required".into());
    }

    let hostname = client_core::machine::hostname_or_unknown();
    let app2 = app.clone();
    let handle2 = auth_handle.clone();
    let relay2 = relay;
    let provider2 = provider;
    let hostname2 = hostname.clone();
    let mc: MultiConfigHandle = app.state::<MultiConfigHandle>().inner().clone();
    let ws_status = app.state::<Arc<WsStatus>>().inner().clone();
    let relay_connected = app
        .state::<Arc<std::sync::atomic::AtomicBool>>()
        .inner()
        .clone();
    let ws_abort = app.state::<Arc<WsAbortHandle>>().inner().clone();
    let pending_auth_relay = app.state::<Arc<PendingAuthRelay>>().inner().clone();

    tauri::async_runtime::spawn(async move {
        // Step 1: Issue a device code so the browser auth page can complete the flow.
        let client = reqwest::Client::new();
        let machine_id = client_core::machine::stable_machine_id();
        let mut dc_body = serde_json::json!({"hostname": hostname2});
        if !machine_id.is_empty() {
            dc_body["machine_id"] = serde_json::Value::String(machine_id.clone());
        }
        let dc_resp = match client
            .post(format!("{}/auth/device-code", relay2))
            .json(&dc_body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                log::error!("sign_in: device-code request failed: {}", e);
                transition(&app2, &handle2, AuthState::LocalOnly);
                return;
            }
        };

        if !dc_resp.status().is_success() {
            log::error!(
                "sign_in: device-code request failed with HTTP {}",
                dc_resp.status()
            );
            transition(&app2, &handle2, AuthState::LocalOnly);
            return;
        }

        let dc: serde_json::Value = match dc_resp.json().await {
            Ok(dc) => dc,
            Err(e) => {
                log::error!("sign_in: device-code parse failed: {}", e);
                transition(&app2, &handle2, AuthState::LocalOnly);
                return;
            }
        };

        let device_code = match dc["device_code"].as_str() {
            Some(code) if !code.is_empty() => code.to_string(),
            _ => {
                log::error!("sign_in: missing device_code in response");
                transition(&app2, &handle2, AuthState::LocalOnly);
                return;
            }
        };
        let user_code = dc["user_code"].as_str().unwrap_or("").to_string();
        let verification_uri = match dc["verification_uri"].as_str() {
            Some(uri) if !uri.is_empty() => uri.to_string(),
            _ => {
                log::error!("sign_in: missing verification_uri in response");
                transition(&app2, &handle2, AuthState::LocalOnly);
                return;
            }
        };

        // Step 2: Open the browser — directly at the provider's OAuth start URL if
        // a provider was specified, otherwise at the relay's provider-selection page.
        let browser_url = if let Some(p) = &provider2 {
            format!(
                "{}/auth/oauth/{}/start?device_code={}",
                relay2, p, user_code
            )
        } else {
            verification_uri
        };
        if let Err(e) = tauri_plugin_opener::open_url(&browser_url, None::<&str>) {
            log::error!("sign_in: failed to open browser: {}", e);
            transition(&app2, &handle2, AuthState::LocalOnly);
            return;
        }

        // Record the relay URL we opened the browser for. The deep-link handler's
        // else-branch checks this before accepting a cinch://auth/callback (Finding 1).
        pending_auth_relay.set(relay2.clone());

        // Step 3: Poll until the user completes OAuth. Tight cadence early
        // (1s) so OAuth completion is caught quickly; back off to 3s after
        // 20s if the user is taking their time in the browser.
        let poll_url = format!("{}/auth/device-code/poll?code={}", relay2, device_code);
        let started = tokio::time::Instant::now();
        let deadline = started + std::time::Duration::from_secs(5 * 60);
        let fast_window = std::time::Duration::from_secs(20);

        loop {
            let interval = if started.elapsed() < fast_window {
                std::time::Duration::from_secs(1)
            } else {
                std::time::Duration::from_secs(3)
            };
            tokio::time::sleep(interval).await;

            if tokio::time::Instant::now() > deadline {
                log::warn!("sign_in: device-code poll timed out");
                pending_auth_relay.clear();
                transition(&app2, &handle2, AuthState::LocalOnly);
                return;
            }

            let resp = match client
                .get(&poll_url)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("sign_in: poll error: {}", e);
                    continue;
                }
            };

            // 410 Gone means the code expired server-side.
            if resp.status() == reqwest::StatusCode::GONE {
                log::warn!("sign_in: device code expired");
                pending_auth_relay.clear();
                transition(&app2, &handle2, AuthState::LocalOnly);
                return;
            }

            let data: serde_json::Value = match resp.json().await {
                Ok(d) => d,
                Err(e) => {
                    log::warn!("sign_in: poll parse error: {}", e);
                    continue;
                }
            };

            if data["status"].as_str() != Some("complete") {
                continue;
            }

            let token = data["token"].as_str().unwrap_or("").to_string();
            let user_id = data["user_id"].as_str().unwrap_or("").to_string();
            let device_id = data["device_id"].as_str().unwrap_or("").to_string();

            if token.is_empty() || user_id.is_empty() || device_id.is_empty() {
                log::warn!("sign_in: poll returned incomplete credentials");
                continue;
            }

            log::info!(
                "sign_in: poll complete — token_prefix={}, token_len={}, user_id={}, device_id={}",
                &token.chars().take(8).collect::<String>(),
                token.len(),
                user_id,
                device_id,
            );

            // Write credentials atomically: token + AES key + X25519 device
            // key + config in one transaction with a single credential_version
            // bump. The CLI watcher (and our own propagate.rs) only see a
            // fully-formed credential set on the bump.
            if let Err(e) = client_core::auth_session::install_credentials(
                client_core::auth_session::InstallParams {
                    user_id: &user_id,
                    device_id: &device_id,
                    token: &token,
                    relay_url: &relay2,
                    hostname: &hostname2,
                    device_private_key: None,
                    email: "",
                    identity_provider: "",
                    display_name: "",
                },
            ) {
                log::error!("sign_in: install_credentials failed: {}", e);
                pending_auth_relay.clear();
                transition(&app2, &handle2, AuthState::LocalOnly);
                return;
            }

            // Reload MultiConfig to get the active relay_id.
            let active_relay_id = match crate::auth::load_multi_config() {
                Ok(new_mc) => {
                    let id = new_mc.active_relay_id.clone().unwrap_or_default();
                    *mc.lock().unwrap() = new_mc;
                    id
                }
                Err(e) => {
                    log::error!("sign_in: load_multi_config failed: {}", e);
                    String::new()
                }
            };

            transition(
                &app2,
                &handle2,
                AuthState::Authenticated {
                    user_id: user_id.clone(),
                    device_id: device_id.clone(),
                    hostname: hostname2.clone(),
                    relay_url: relay2.clone(),
                    active_relay_id: active_relay_id.clone(),
                    machine_id: machine_id.clone(),
                },
            );

            // Joiner bootstrap runs concurrently so the WS connection is not delayed.
            // If a bearer responds within 30s, the canonical AES key overwrites the
            // locally-generated placeholder; subsequent decrypt attempts use the right key.
            let bs_relay = relay2.clone();
            let bs_token = token.clone();
            let bs_user = user_id.clone();
            let bs_device = device_id.clone();
            tokio::spawn(async move {
                crate::auth_bootstrap::run_joiner_flow(&bs_relay, &bs_token, &bs_user, &bs_device)
                    .await;
            });

            // Restart the client-core Writer with the new credentials.
            {
                let app3 = app2.clone();
                let rw_relay = relay2.clone();
                let rw_token = token.clone();
                let rw_ws_status = ws_status.clone();
                let rw_relay_connected = relay_connected.clone();
                let jh = tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::restart_writer(
                        &app3,
                        &rw_relay,
                        &rw_token,
                        &rw_ws_status,
                        &rw_relay_connected,
                    )
                    .await
                    {
                        log::error!("sign_in: restart_writer failed: {}", e);
                    }
                });
                ws_abort.replace(jh);
            }

            // I1: Clear pending state — login completed via polling, deep-link no longer needed.
            pending_auth_relay.clear();
            log::info!(
                "sign_in: complete via polling: user={}, device={}, relay_id={}",
                user_id,
                device_id,
                active_relay_id,
            );
            return;
        }
    });

    Ok(())
}
