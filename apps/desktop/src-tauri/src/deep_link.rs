//! Hot-app deep-link handler: cinch://login (CLI handoff) and
//! cinch://auth/callback (browser → app token delivery).
//!
//! The cold-start case (app launched via URL) is handled by the React side
//! calling `handle_deeplink` via `getCurrent()`; this module wires the
//! handler used while the app is already running.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri_specta::Event;

use crate::auth::{self, AuthStateHandle};
use crate::commands;
use crate::protocol::MultiConfigHandle;
use crate::sync_status;
use crate::validate::{validate_auth_callback, validate_relay_url};
use crate::writer_restart::restart_writer;

#[allow(clippy::too_many_arguments)]
pub(crate) fn install_deep_link_handler(
    app: &tauri::App,
    auth_state_handle: AuthStateHandle,
    ws_status: Arc<sync_status::WsStatus>,
    relay_connected: Arc<AtomicBool>,
    multi_config_handle: MultiConfigHandle,
    ws_abort_handle: Arc<sync_status::WsAbortHandle>,
    pending_relay_add: Arc<commands::relays::PendingRelayAdd>,
    pending_auth_relay: Arc<commands::relays::PendingAuthRelay>,
) {
    use tauri_plugin_deep_link::DeepLinkExt;

    let dl_auth_handle = auth_state_handle;
    let dl_app_handle = app.handle().clone();
    let dl_ws_status = ws_status;
    let dl_relay_connected = relay_connected;
    let dl_mc = multi_config_handle;
    let dl_ws_abort = ws_abort_handle;
    let dl_pending = pending_relay_add;
    let dl_pending_auth = pending_auth_relay;
    app.deep_link().on_open_url(move |event| {
        let urls = event.urls();
        for url in &urls {
            // CLI handoff route: `cinch://login?relay=…&from=cli`.
            // Focus the main window and emit an event so the React
            // layer opens the AddRelayDialog with the relay
            // pre-filled. No credential write here — the user
            // still has to complete OAuth in the dialog.
            let is_login = url.host_str() == Some("login")
                || url.path() == "/login"
                || (url.scheme() == "cinch" && url.path() == "/login");
            if is_login {
                crate::show_on_active_monitor(&dl_app_handle);
                let relay = url
                    .query_pairs()
                    .find(|(k, _)| k == "relay")
                    .map(|(_, v)| v.to_string())
                    .unwrap_or_default();
                if let Err(e) =
                    (crate::events::CliHandoffRequested { relay_url: relay }).emit(&dl_app_handle)
                {
                    log::warn!("emit CliHandoffRequested failed: {}", e);
                }
                continue;
            }

            let is_auth = url.host_str() == Some("auth") || url.path() == "/auth/callback";
            if !is_auth {
                continue;
            }

            let token = url
                .query_pairs()
                .find(|(k, _)| k == "token")
                .map(|(_, v)| v.to_string());
            let device_id = url
                .query_pairs()
                .find(|(k, _)| k == "device_id")
                .map(|(_, v)| v.to_string());
            let user_id = url
                .query_pairs()
                .find(|(k, _)| k == "user_id")
                .map(|(_, v)| v.to_string());
            let relay_url = url
                .query_pairs()
                .find(|(k, _)| k == "relay_url")
                .map(|(_, v)| v.to_string());

            if let (Some(token), Some(device_id), Some(user_id)) = (token, device_id, user_id) {
                if token.len() != 64 || !token.chars().all(|c| c.is_ascii_hexdigit()) {
                    log::warn!("deep-link: rejected malformed token");
                    return;
                }

                let relay = relay_url.unwrap_or_else(|| "https://api.cinchcli.com".to_string());

                if let Err(e) = validate_relay_url(&relay) {
                    log::warn!("deep-link: rejected invalid relay_url: {}", e);
                    return;
                }

                let hostname = client_core::machine::hostname_or_unknown();

                let pending_info = dl_pending.take();
                let active_relay_id = if let Some(info) = pending_info {
                    match auth::add_relay_profile(
                        &user_id,
                        &device_id,
                        &token,
                        &relay,
                        &hostname,
                        info.label.as_deref(),
                        "",
                    ) {
                        Ok(relay_id) => {
                            if let Ok(new_mc) = auth::load_multi_config() {
                                let mut g = dl_mc.lock().unwrap();
                                *g = new_mc;
                            }
                            relay_id
                        }
                        Err(e) => {
                            log::error!("deep-link add_relay_profile failed: {}", e);
                            return;
                        }
                    }
                } else {
                    // Security: require a pending standard-auth relay URL that
                    // matches the callback. Rejects crafted deep-links that
                    // arrive with no prior login being initiated (Finding 1).
                    // I3: peek first so a junk deep-link cannot consume the
                    // pending state before the legitimate callback arrives.
                    let pending_auth_url = dl_pending_auth.peek();
                    if let Err(reason) = validate_auth_callback(pending_auth_url.as_deref(), &relay)
                    {
                        log::warn!("deep-link: {}", reason);
                        return;
                    }
                    // Validation passed — now consume the pending state.
                    dl_pending_auth.clear();

                    if let Err(e) = client_core::auth_session::install_credentials(
                        client_core::auth_session::InstallParams {
                            user_id: &user_id,
                            device_id: &device_id,
                            token: &token,
                            relay_url: &relay,
                            hostname: &hostname,
                            device_private_key: None,
                            email: "",
                            identity_provider: "",
                            display_name: "",
                        },
                    ) {
                        log::error!("deep-link install_credentials failed: {}", e);
                        return;
                    }
                    auth::load_multi_config()
                        .ok()
                        .and_then(|mc| {
                            let id = mc.active_relay_id.clone();
                            let mut g = dl_mc.lock().unwrap();
                            *g = mc;
                            id
                        })
                        .unwrap_or_default()
                };

                auth::transition(
                    &dl_app_handle,
                    &dl_auth_handle,
                    auth::AuthState::Authenticated {
                        user_id: user_id.clone(),
                        device_id: device_id.clone(),
                        hostname: hostname.clone(),
                        relay_url: relay.clone(),
                        active_relay_id: active_relay_id.clone(),
                        machine_id: client_core::machine::stable_machine_id(),
                    },
                );

                // Restart the client-core Writer with the new credentials.
                let app_for_writer = dl_app_handle.clone();
                let writer_relay = relay.clone();
                let writer_token = token.clone();
                let dl_ws_status2 = dl_ws_status.clone();
                let dl_relay_connected2 = dl_relay_connected.clone();
                let jh = tauri::async_runtime::spawn(async move {
                    if let Err(e) = restart_writer(
                        &app_for_writer,
                        &writer_relay,
                        &writer_token,
                        &dl_ws_status2,
                        &dl_relay_connected2,
                    )
                    .await
                    {
                        log::error!("deep-link: restart_writer failed: {}", e);
                    }
                });
                dl_ws_abort.replace(jh);

                log::info!(
                    "deep-link auth complete: user={}, device={}, relay_id={}",
                    user_id,
                    device_id,
                    active_relay_id,
                );
            }
        }
    });
}
