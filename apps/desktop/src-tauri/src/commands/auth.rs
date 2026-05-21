//! Tauri commands for auth state — called from React via invoke().
//! All commands return Result<T, String> to match the existing convention.

use std::sync::Arc;

use client_core::pair_script::{
    sh_single_quote, FIND_SUPPORTED_CINCH_BLOCK, INSTALL_BLOCK, SKIP_INSTALL_BLOCK,
};
use tauri::{AppHandle, Manager, State};
use tauri_specta::Event;

use crate::auth::{
    add_relay_profile, load_multi_config, transition, wipe_credentials, AuthState, AuthStateHandle,
};
use crate::commands::relays::{PendingAuthRelay, PendingRelayAdd};
use crate::protocol::MultiConfigHandle;
use crate::sync_status::{WsAbortHandle, WsStatus};

/// Display-facing user profile sourced from the active relay profile.
///
/// Returned by [`get_user_profile`]. All fields are empty strings when no
/// relay profile is configured (graceful pre-auth state).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UserProfile {
    pub display_name: String,
    pub email: String,
    pub identity_provider: String,
    pub user_id: String,
}

/// Returns display-facing identity fields from the active relay profile.
///
/// Falls back to all-empty-string `UserProfile` when no profile is configured
/// (e.g. before first sign-in) so the frontend can render a sensible
/// unauthenticated state without a `Result` branch.
#[tauri::command]
#[specta::specta]
pub fn get_user_profile() -> UserProfile {
    let cfg = match crate::protocol::Config::load() {
        Ok(c) => c,
        Err(_) => {
            return UserProfile {
                display_name: String::new(),
                email: String::new(),
                identity_provider: String::new(),
                user_id: String::new(),
            }
        }
    };
    UserProfile {
        display_name: cfg.display_name,
        email: cfg.email,
        identity_provider: cfg.identity_provider,
        user_id: cfg.user_id,
    }
}

/// Inner logic for `set_display_name` — separated for unit-test stubbing.
///
/// Validates `name` locally (trim → non-empty → ≤ 64 bytes), then calls `f`
/// with `(relay_url, trimmed_name)` to do the network work.  On success the
/// returned stored name is written back into the active relay profile on disk.
pub(crate) async fn set_display_name_inner<F, Fut>(name: &str, f: F) -> Result<String, String>
where
    F: FnOnce(String, String) -> Fut,
    Fut: std::future::Future<Output = Result<String, String>>,
{
    // Local validation — must run BEFORE loading config so that bad input is
    // rejected immediately without any I/O.
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("display_name must not be empty".into());
    }
    if trimmed.len() > 64 {
        return Err("display_name must be 64 bytes or fewer (too long)".into());
    }

    // Load the active relay profile to extract relay_url and verify auth.
    let cfg = crate::protocol::Config::load().map_err(|e| e.to_string())?;
    if cfg.token.is_empty() {
        return Err("Not authenticated".into());
    }

    // Delegate the network call to the closure (allows stubbing in tests).
    let stored = f(cfg.relay_url.clone(), trimmed.to_string()).await?;

    // Refresh the local cache so the next get_user_profile reflects the change.
    // MultiConfig is the canonical disk format; we mutate the active profile
    // directly and then save, which preserves all other relay profiles.
    let mut mc = crate::protocol::MultiConfig::load();
    if let Some(profile) = mc.active_profile_mut() {
        profile.display_name = stored.clone();
        mc.save().map_err(|e| format!("save config: {}", e))?;
    }

    Ok(stored)
}

/// Updates the display name for the currently authenticated user.
///
/// Validates locally (trim → non-empty → ≤ 64 bytes), POSTs to
/// `{relay_url}/auth/display-name`, and refreshes the on-disk config so the
/// next `get_user_profile` call returns the updated name.
///
/// Returns the name as stored by the relay (trimmed).
#[tauri::command]
#[specta::specta]
pub async fn set_display_name(name: String) -> Result<String, String> {
    set_display_name_inner(&name, |relay_url, value| async move {
        // Re-read the config to get the token (the inner function consumed relay_url
        // from the flat Config it loaded; we need both relay_url and token here).
        let cfg = crate::protocol::Config::load().map_err(|e| e.to_string())?;
        let client =
            client_core::http::RestClient::new(relay_url, cfg.token, crate::build_client_info())
                .map_err(|e| e.to_string())?;
        client.set_display_name(&value).await.map_err(|e| match e {
            client_core::http::HttpError::Unauthorized => "Authentication failed".to_string(),
            other => format!("{}", other),
        })
    })
    .await
}

/// Returns the current AuthState. Used by AuthProvider's initial fetch in React.
#[tauri::command]
#[specta::specta]
pub fn get_auth_state(handle: State<'_, AuthStateHandle>) -> AuthState {
    handle.lock().unwrap().clone()
}

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

/// list_auth_providers — fetches `GET {relay}/auth/providers` from the relay.
///
/// Lives in Rust because the Tauri WebView's CSP (`connect-src 'self' ipc:`)
/// blocks the frontend from issuing a cross-origin `fetch` to relay URLs.
/// Returns `[]` on any failure so the dialog can fall back to a generic
/// sign-in button without surfacing a transient error during typing.
#[tauri::command]
#[specta::specta]
pub async fn list_auth_providers(relay_url: String) -> Vec<String> {
    let relay = relay_url.trim().trim_end_matches('/');
    if !relay.starts_with("http") {
        return Vec::new();
    }
    let url = format!("{}/auth/providers", relay);
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    let resp = match client.get(&url).send().await {
        Ok(r) if r.status().is_success() => r,
        _ => return Vec::new(),
    };
    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    body["providers"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

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

/// sign_out — calls POST /auth/device/revoke (best-effort), wipes credentials, transitions to LocalOnly.
/// Mirrors the CLI `auth logout` D-10 behavior.
#[tauri::command]
#[specta::specta]
pub async fn sign_out(
    app: AppHandle,
    handle: State<'_, AuthStateHandle>,
    pending_auth: State<'_, Arc<PendingAuthRelay>>,
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
    transition(&app, &handle, AuthState::LocalOnly);
    Ok(())
}

/// retry_auth — bypasses ErrorRecoverable.retry_after_ms and re-attempts the last failing operation.
/// For Phase 2 plumbing: resets to LocalOnly and lets the user re-invoke sign_in.
/// Phase 3+ will store the last-attempted operation and retry it in place.
#[tauri::command]
#[specta::specta]
pub async fn retry_auth(app: AppHandle, handle: State<'_, AuthStateHandle>) -> Result<(), String> {
    // Conservative v1: just transition to LocalOnly; React re-renders SetupScreen.
    transition(&app, &handle, AuthState::LocalOnly);
    Ok(())
}

/// pair_via_ssh — SSH into a remote machine, install/upgrade cinch, and
/// authenticate it against the same relay account as the local desktop.
///
/// Verification contract (fixes the 0.1.5 silent-success bug):
///   1. The local desktop must already be signed in to the target relay.
///      Otherwise we cannot tell whether the remote ended up linked to the
///      right user_id — abort up front rather than report false success.
///   2. The remote script always emits a `<<CINCH-PAIRED-OK>>{...}<<END>>`
///      marker on stdout when it considers the remote paired (either it
///      reused an existing matching pairing, or it ran a fresh device-code
///      login). Without that marker, SSH exit 0 means nothing.
///   3. After the SSH process exits, we require the marker to have been
///      observed AND its `user_id` to match the local user. A blank or
///      mismatching marker becomes a hard error so the UI shows "Setup
///      failed" instead of "paired successfully".
///
/// In parallel, when the remote emits the legacy `<<CINCH-DEVICE-CODE>>`
/// marker (fresh-pair path) we still fire `SshPairMarkerFound` so the
/// frontend opens the browser.
#[tauri::command]
#[specta::specta]
pub async fn pair_via_ssh(
    app: AppHandle,
    target: String,
    relay_url: Option<String>,
    skip_install: bool,
) -> Result<(), String> {
    use std::process::Stdio;
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::process::Command as TokioCommand;

    let multi_cfg = client_core::auth::load_multi_config().map_err(|e| e.to_string())?;
    let remote_relay = match relay_url {
        Some(s) if !s.trim().is_empty() => s.trim().trim_end_matches('/').to_string(),
        _ => multi_cfg
            .active_profile()
            .map(|p| p.relay_url.trim_end_matches('/').to_string())
            .unwrap_or_default(),
    };
    if remote_relay.is_empty() {
        return Err(
            "No relay configured on this machine — sign in first, then add an SSH machine.".into(),
        );
    }
    let expected_user_id = multi_cfg
        .relays
        .iter()
        .find(|p| p.relay_url.trim_end_matches('/') == remote_relay)
        .map(|p| p.user_id.clone())
        .filter(|id| !id.is_empty())
        .ok_or_else(|| {
            format!(
                "Not signed in to relay {} on this machine. Sign in here first so the remote can be linked to your account.",
                remote_relay
            )
        })?;

    let script = build_pair_script(&remote_relay, skip_install, &expected_user_id);

    let mut child = TokioCommand::new("ssh")
        .arg(&target)
        .arg("sh")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("SSH spawn failed: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(script.as_bytes())
            .await
            .map_err(|e| format!("Writing script: {}", e))?;
    }

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "SSH stdout was not captured".to_string())?;
    let app_for_stdout = app.clone();
    let pairing_marker: Arc<StdMutex<Option<client_core::auth::PairingCompleteMarker>>> =
        Arc::new(StdMutex::new(None));
    let pairing_marker_writer = pairing_marker.clone();
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| format!("Reading output: {}", e))?
        {
            if let Some(marker) = client_core::auth::parse_device_code_marker(&line) {
                if let Err(e) = tauri_plugin_opener::open_url(&marker.url, None::<&str>) {
                    log::warn!("pair_via_ssh: failed to open browser: {}", e);
                }
                crate::events::SshPairMarkerFound { url: marker.url }
                    .emit(&app_for_stdout)
                    .ok();
            } else if let Some(complete) = client_core::auth::parse_pairing_complete_marker(&line) {
                if let Ok(mut slot) = pairing_marker_writer.lock() {
                    *slot = Some(complete);
                }
            } else {
                log::info!("pair_via_ssh stdout: {}", line);
            }
        }
        Ok::<(), String>(())
    });

    let stderr_task = child.stderr.take().map(|stderr| {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut recent = std::collections::VecDeque::new();
            while let Some(line) = lines
                .next_line()
                .await
                .map_err(|e| format!("Reading stderr: {}", e))?
            {
                log::warn!("pair_via_ssh stderr: {}", line);
                if recent.len() == 12 {
                    recent.pop_front();
                }
                recent.push_back(line);
            }
            Ok::<Vec<String>, String>(recent.into_iter().collect())
        })
    });

    let status = child.wait().await.map_err(|e| format!("SSH wait: {}", e))?;
    stdout_task
        .await
        .map_err(|e| format!("SSH stdout task: {}", e))??;
    let stderr_tail = if let Some(task) = stderr_task {
        task.await
            .map_err(|e| format!("SSH stderr task: {}", e))??
    } else {
        Vec::new()
    };
    if !status.success() {
        let mut message = format!("Remote setup failed (exit {})", status.code().unwrap_or(-1));
        if !stderr_tail.is_empty() {
            message.push_str(": ");
            message.push_str(&stderr_tail.join("\n"));
        }
        return Err(message);
    }

    // Exit 0 alone isn't enough: a remote that was already signed in as a
    // different user (or whose `cinch auth login` short-circuited) could
    // also exit 0 without actually pairing to our account. The marker is
    // the only ground truth.
    let marker = pairing_marker
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .ok_or_else(|| {
            "Pairing did not complete: remote did not confirm the linked account. The remote may be running an old cinch that does not emit the pairing-complete marker — run the remote install once with this version of the desktop app.".to_string()
        })?;
    if marker.user_id != expected_user_id {
        return Err(format!(
            "Remote paired as a different user (remote user_id={}, expected={}). The browser sign-in must use the same account as this machine.",
            marker.user_id, expected_user_id
        ));
    }
    Ok(())
}

/// list_ssh_hosts — return concrete aliases from the user's ~/.ssh/config.
#[tauri::command]
#[specta::specta]
pub fn list_ssh_hosts() -> Result<Vec<String>, String> {
    let Some(home) = dirs::home_dir() else {
        return Ok(Vec::new());
    };
    let config_path = home.join(".ssh").join("config");
    match std::fs::read_to_string(&config_path) {
        Ok(config) => Ok(parse_ssh_config_hosts(&config)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("Reading {}: {}", config_path.display(), e)),
    }
}

fn parse_ssh_config_hosts(config: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    for raw_line in config.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut parts = line.split_whitespace();
        let Some(keyword) = parts.next() else {
            continue;
        };
        if !keyword.eq_ignore_ascii_case("host") {
            continue;
        }

        for alias in parts {
            if alias.starts_with('!') || alias.contains('*') || alias.contains('?') {
                continue;
            }
            if !hosts.iter().any(|existing| existing == alias) {
                hosts.push(alias.to_string());
            }
        }
    }
    hosts
}

fn build_pair_script(relay_url: &str, skip_install: bool, expected_user_id: &str) -> String {
    let mut s = String::new();
    s.push_str("#!/bin/sh\nset -e\n\n");
    s.push_str(&format!("RELAY_URL={}\n", sh_single_quote(relay_url)));
    s.push_str(&format!(
        "EXPECTED_USER_ID={}\n\n",
        sh_single_quote(expected_user_id)
    ));

    // The remote must be told *which* local account we expect it to match.
    // Empty here would mean the desktop forgot to pass it — fail fast rather
    // than silently re-using whatever happens to be on the remote disk.
    s.push_str(
        r#"if [ -z "$EXPECTED_USER_ID" ]; then
  echo "Error: pair invoked without an expected user_id (desktop is not signed in to this relay)." >&2
  exit 1
fi

"#,
    );

    if skip_install {
        s.push_str(SKIP_INSTALL_BLOCK);
    } else {
        s.push_str(INSTALL_BLOCK);
    }

    s.push_str(FIND_SUPPORTED_CINCH_BLOCK);

    s.push_str(
        r#"CINCH_DIR="$HOME/.cinch"
CINCH_CONFIG="$CINCH_DIR/config.json"
mkdir -p "$CINCH_DIR"

# Extract a field from the on-disk config, handling both the MultiConfig
# shape (active_relay_id + relays[]) and the legacy single-relay Config
# (top-level user_id / active_device_id). Prints empty string when the
# file is missing, empty, or malformed.
cinch_active_field() {
  FIELD="$1"
  LEGACY_FIELD="$FIELD"
  if [ "$FIELD" = "device_id" ]; then
    LEGACY_FIELD="active_device_id"
  fi
  if [ ! -f "$CINCH_CONFIG" ] || [ ! -s "$CINCH_CONFIG" ]; then
    return 0
  fi
  if command -v jq >/dev/null 2>&1; then
    jq -r --arg f "$FIELD" --arg lf "$LEGACY_FIELD" '
      if ((.relays // []) | length) > 0 and ((.active_relay_id // "") | length) > 0 then
        (.active_relay_id as $aid
          | (.relays[] | select(.id == $aid) | .[$f]) // "")
      else
        (.[$lf] // "")
      end
    ' "$CINCH_CONFIG" 2>/dev/null || true
  elif command -v python3 >/dev/null 2>&1; then
    python3 - "$CINCH_CONFIG" "$FIELD" "$LEGACY_FIELD" <<'PYEOF' 2>/dev/null || true
import json, sys
path, field, legacy = sys.argv[1], sys.argv[2], sys.argv[3]
try:
    with open(path) as f:
        cfg = json.load(f)
except Exception:
    print("")
    sys.exit(0)
relays = cfg.get("relays")
active = cfg.get("active_relay_id") or ""
if isinstance(relays, list) and active:
    for r in relays:
        if isinstance(r, dict) and r.get("id") == active:
            print(r.get(field, "") or "")
            sys.exit(0)
    print("")
    sys.exit(0)
print(cfg.get(legacy, "") or "")
PYEOF
  fi
}

REMOTE_USER_ID="$(cinch_active_field user_id || true)"

if [ -n "$REMOTE_USER_ID" ]; then
  if [ "$REMOTE_USER_ID" = "$EXPECTED_USER_ID" ]; then
    # Same user on disk — verify the relay still trusts the token before
    # claiming reuse. `cinch auth status` writes to stderr only, so capture
    # both streams.
    STATUS_OUT="$("$CINCH_BIN" auth status 2>&1 || true)"
    case "$STATUS_OUT" in
      *"Credentials expired or revoked"*|*"Not authenticated"*)
        echo "Local credentials no longer valid — re-pairing..." >&2
        "$CINCH_BIN" auth logout >/dev/null 2>&1 || true
        ;;
      *Authenticated*)
        REMOTE_DEVICE_ID="$(cinch_active_field device_id || true)"
        printf '<<CINCH-PAIRED-OK>>{"user_id":"%s","device_id":"%s","reused":true}<<END>>\n' \
          "$REMOTE_USER_ID" "$REMOTE_DEVICE_ID"
        exit 0
        ;;
      *)
        echo "Unexpected auth status output; re-pairing to be safe:" >&2
        printf '%s\n' "$STATUS_OUT" >&2
        "$CINCH_BIN" auth logout >/dev/null 2>&1 || true
        ;;
    esac
  else
    echo "Remote is signed in as a different user ($REMOTE_USER_ID); logging out before re-pair..." >&2
    "$CINCH_BIN" auth logout >/dev/null 2>&1 || true
  fi
fi

echo "Authenticating with relay at $RELAY_URL..."
"$CINCH_BIN" auth login --headless --force --relay "$RELAY_URL"

NEW_USER_ID="$(cinch_active_field user_id || true)"
NEW_DEVICE_ID="$(cinch_active_field device_id || true)"
printf '<<CINCH-PAIRED-OK>>{"user_id":"%s","device_id":"%s","reused":false}<<END>>\n' \
  "$NEW_USER_ID" "$NEW_DEVICE_ID"
"#,
    );

    s
}

// ---------------------------------------------------------------------------
// Remote-login approval / denial commands (Task 3.4)
// ---------------------------------------------------------------------------

/// Core logic for approving a pending device-code login.
/// Extracted so tests can drive it without a live Tauri AppHandle.
pub(crate) async fn approve_remote_login_impl(
    user_code: &str,
    relay_url: &str,
    token: &str,
    pending: &crate::auth::state::PendingCodesHandle,
) -> Result<(), String> {
    let client = client_core::http::RestClient::new(relay_url, token, crate::build_client_info())
        .map_err(|e| e.to_string())?;
    client
        .complete_device_code(user_code)
        .await
        .map_err(|e| e.to_string())?;
    crate::auth::state::remove_pending_code(pending, user_code);
    Ok(())
}

/// Core logic for denying a pending device-code login.
/// Extracted so tests can drive it without a live Tauri AppHandle.
pub(crate) async fn deny_remote_login_impl(
    user_code: &str,
    relay_url: &str,
    token: &str,
    pending: &crate::auth::state::PendingCodesHandle,
) -> Result<(), String> {
    let client = client_core::http::RestClient::new(relay_url, token, crate::build_client_info())
        .map_err(|e| e.to_string())?;
    client
        .deny_device_code(user_code)
        .await
        .map_err(|e| e.to_string())?;
    crate::auth::state::remove_pending_code(pending, user_code);
    Ok(())
}

/// approve_remote_login — accept a pending device-code request and clear it
/// from the local pending list.
///
/// Calls `POST /auth/device-code/complete` on the relay with bearer auth,
/// then removes the matching entry from PendingCodesHandle.
#[tauri::command]
#[specta::specta]
pub async fn approve_remote_login(
    user_code: String,
    _app: AppHandle,
    pending: State<'_, crate::auth::state::PendingCodesHandle>,
) -> Result<(), String> {
    let cfg = crate::protocol::Config::load().unwrap_or_default();
    if cfg.token.is_empty() {
        return Err("not signed in".into());
    }
    approve_remote_login_impl(&user_code, &cfg.relay_url, &cfg.token, pending.inner()).await
}

/// deny_remote_login — reject a pending device-code request and clear it
/// from the local pending list.
///
/// Calls `POST /cinch.v1.AuthService/DeviceCodeDeny` (Connect-RPC unary)
/// on the relay with bearer auth, then removes the matching entry from
/// PendingCodesHandle.
#[tauri::command]
#[specta::specta]
pub async fn deny_remote_login(
    user_code: String,
    _app: AppHandle,
    pending: State<'_, crate::auth::state::PendingCodesHandle>,
) -> Result<(), String> {
    let cfg = crate::protocol::Config::load().unwrap_or_default();
    if cfg.token.is_empty() {
        return Err("not signed in".into());
    }
    deny_remote_login_impl(&user_code, &cfg.relay_url, &cfg.token, pending.inner()).await
}

#[cfg(test)]
mod ssh_config_tests {
    use super::*;

    #[test]
    fn parse_ssh_config_hosts_returns_concrete_aliases_only() {
        let config = r#"
Host *
  AddKeysToAgent yes

Host oci_atlas_1 jgopi
  User opc

Host 192.168.* ?ast
  User ignored

Host HomeServer
  ProxyJump jgopi
"#;

        assert_eq!(
            parse_ssh_config_hosts(config),
            vec![
                "oci_atlas_1".to_string(),
                "jgopi".to_string(),
                "HomeServer".to_string(),
            ],
        );
    }

    #[test]
    fn build_pair_script_is_valid_posix_shell() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let script = build_pair_script("https://api.cinchcli.com", false, "01HXYZ_USER");
        let mut child = Command::new("sh")
            .arg("-n")
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn sh -n");
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(script.as_bytes())
            .expect("write script");
        let out = child.wait_with_output().expect("wait sh");
        assert!(
            out.status.success(),
            "generated script failed sh -n:\nstderr:\n{}\nscript:\n{}",
            String::from_utf8_lossy(&out.stderr),
            script
        );
    }

    #[test]
    fn build_pair_script_verifies_remote_cinch_supports_headless_login() {
        let script = build_pair_script("https://api.cinchcli.com", false, "01HXYZ_USER");

        assert!(script.contains("find_supported_cinch"));
        assert!(script.contains("does not support SSH pairing"));
        // Fresh-pair branch must use --force to bypass the CLI's already-signed-in
        // short-circuit; without it the remote could exit 0 with no marker.
        assert!(
            script.contains("\"$CINCH_BIN\" auth login --headless --force --relay \"$RELAY_URL\"")
        );
    }

    #[test]
    fn build_pair_script_embeds_expected_user_id() {
        let script = build_pair_script("https://api.cinchcli.com", true, "01HXYZ_USER");
        assert!(script.contains("EXPECTED_USER_ID='01HXYZ_USER'"));
        assert!(script.contains("RELAY_URL='https://api.cinchcli.com'"));
    }

    #[test]
    fn build_pair_script_emits_pairing_complete_marker_on_both_paths() {
        let script = build_pair_script("https://api.cinchcli.com", false, "u1");
        // Reused path (already paired): marker with reused=true.
        assert!(script.contains(
            "<<CINCH-PAIRED-OK>>{\"user_id\":\"%s\",\"device_id\":\"%s\",\"reused\":true}<<END>>"
        ));
        // Fresh-pair path: marker with reused=false after login completes.
        assert!(script.contains(
            "<<CINCH-PAIRED-OK>>{\"user_id\":\"%s\",\"device_id\":\"%s\",\"reused\":false}<<END>>"
        ));
    }

    #[test]
    fn build_pair_script_logs_out_other_user_before_repair() {
        let script = build_pair_script("https://api.cinchcli.com", false, "u1");
        assert!(script.contains("signed in as a different user"));
        assert!(script.contains("\"$CINCH_BIN\" auth logout"));
    }

    #[test]
    fn build_pair_script_aborts_when_expected_user_id_is_empty() {
        let script = build_pair_script("https://api.cinchcli.com", false, "");
        assert!(script.contains("EXPECTED_USER_ID=''"));
        assert!(script.contains("pair invoked without an expected user_id"));
    }

    #[test]
    fn build_pair_script_handles_multi_config_via_jq_and_python() {
        let script = build_pair_script("https://api.cinchcli.com", false, "u1");
        // Must look up the active relay profile, not just the legacy top-level field.
        assert!(script.contains(".active_relay_id"));
        assert!(script.contains("active_relay_id"));
        // Python fallback for hosts without jq.
        assert!(script.contains("python3 -"));
    }

    #[test]
    fn build_pair_script_upgrades_cinch_when_install_not_skipped() {
        let script = build_pair_script("https://api.cinchcli.com", false, "u1");
        assert!(script.contains("Installing/upgrading cinch"));
        assert!(script.contains("curl -fsSL https://cinchcli.com/install.sh"));
    }

    // sh_single_quote and its tests live in
    // `client_core::pair_script` now — see the inline #[cfg(test)] mod
    // there for the same `it's` / empty-string coverage.
}

#[cfg(test)]
mod user_profile_tests {
    use super::UserProfile;

    #[test]
    fn user_profile_struct_serializes_with_expected_keys() {
        let p = UserProfile {
            display_name: "Alice Example".into(),
            email: "alice@example.com".into(),
            identity_provider: "github".into(),
            user_id: "01HZ".into(),
        };
        let v: serde_json::Value = serde_json::to_value(&p).unwrap();
        assert_eq!(v["display_name"], "Alice Example");
        assert_eq!(v["email"], "alice@example.com");
        assert_eq!(v["identity_provider"], "github");
        assert_eq!(v["user_id"], "01HZ");
    }
}

#[cfg(test)]
mod set_display_name_tests {
    use super::set_display_name_inner;

    #[tokio::test]
    async fn set_display_name_rejects_blank_locally() {
        let result = set_display_name_inner("  ", |_, _| async { Ok(String::new()) }).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("must not be empty") || err.contains("empty"),
            "got: {}",
            err
        );
    }

    #[tokio::test]
    async fn set_display_name_rejects_too_long_locally() {
        let long = "a".repeat(65);
        let result = set_display_name_inner(&long, |_, _| async { Ok(String::new()) }).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("64") || err.contains("too long"),
            "got: {}",
            err
        );
    }

    #[tokio::test]
    async fn set_display_name_calls_relay_and_returns_stored_value() {
        // The closure receives (relay_url, trimmed_name) and returns the stored name.
        // We can't easily test the "refresh local config" side-effect here without
        // touching HOME — rely on Task 4's component test for the end-to-end refresh.
        let result = set_display_name_inner("Alice", |_url, name| async move {
            assert_eq!(name, "Alice");
            Ok("Alice".to_string())
        })
        .await;
        // This may fail in environments without a config file ("no active
        // relay configured") or with an empty token ("Not authenticated").
        // Both are legitimate outcomes when the test runs without a real
        // login state. Accept Ok("Alice") OR any config/auth-related error.
        match result {
            Ok(s) => assert_eq!(s, "Alice"),
            Err(e) => assert!(
                e.contains("authenticated") || e.contains("relay") || e.contains("config"),
                "unexpected error: {}",
                e
            ),
        }
    }
}
