use std::sync::Arc;

use tauri::State;

use crate::protocol::{ConfigInfo, MultiConfigHandle};
use crate::sync_status::WsStatus;

// ---------------------------------------------------------------------------
// Config / auth info — no store dependency
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn get_config_info(mc: State<'_, MultiConfigHandle>) -> ConfigInfo {
    let guard = mc.lock().unwrap();
    let cfg = guard.to_active_config();
    ConfigInfo {
        relay_url: cfg.relay_url.clone(),
        user_id: cfg.user_id.clone(),
        hostname: cfg.hostname.clone(),
    }
}

// ---------------------------------------------------------------------------
// save_config — no store dependency
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn save_config(app: tauri::AppHandle, relay_url: String, token: String) -> Result<(), String> {
    if relay_url.trim().is_empty() || token.trim().is_empty() {
        return Err("relay_url and token are required".to_string());
    }
    let relay_url = relay_url.trim().trim_end_matches('/').to_string();
    let hostname = client_core::machine::hostname_or_unknown();

    let existing = crate::protocol::Config::load().unwrap_or_default();
    let user_id = if existing.user_id.is_empty() {
        "unknown-user".to_string()
    } else {
        existing.user_id
    };
    let device_id = if existing.active_device_id.is_empty() {
        "unknown-device".to_string()
    } else {
        existing.active_device_id
    };

    crate::auth::credential::write_credentials(
        &user_id,
        &device_id,
        token.trim(),
        &relay_url,
        &hostname,
    )
    .map_err(|e| format!("write_credentials: {}", e))?;

    log::info!("save_config succeeded");

    app.restart();
}

// ---------------------------------------------------------------------------
// WS status — no store dependency
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn get_ws_status(ws_status: State<'_, Arc<WsStatus>>) -> String {
    ws_status.get()
}

// ---------------------------------------------------------------------------
// Focus previous app — no store dependency
// ---------------------------------------------------------------------------

/// Restore focus to the app that was frontmost before Cinch was shown, then hide the
/// Cinch window. On non-macOS platforms this simply hides the window.
#[tauri::command]
#[specta::specta]
// `previous_pid` is only read inside the `#[cfg(target_os = "macos")]` block
// below; on Linux/Windows the param exists purely to keep the Tauri command
// signature stable, so suppress the unused-var lint there.
#[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
pub fn focus_previous_app(
    app: tauri::AppHandle,
    previous_pid: State<'_, crate::PreviousAppPid>,
) -> Result<(), String> {
    use tauri::Manager;
    #[cfg(target_os = "macos")]
    {
        let pid_opt = *previous_pid.lock().map_err(|e| e.to_string())?;
        if let Some(pid) = pid_opt {
            crate::activate_app_by_pid(pid);
        }
    }

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
        crate::window_manage::set_dock_visible(&app, false);
    }

    Ok(())
}
