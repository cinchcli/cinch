use log::info;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{TrayIcon, TrayIconBuilder},
    AppHandle, Manager,
};

use crate::auth::state::AuthState;

pub struct TrayMenuItems {
    pub status: MenuItem<tauri::Wry>,
    pub pending: MenuItem<tauri::Wry>,
    // Kept alive so the system tray icon isn't removed when this scope ends.
    #[allow(dead_code)]
    pub tray: TrayIcon<tauri::Wry>,
}

/// Pure label producer for the first tray row (account + connection status).
/// `ws` is the current `WsStatus` value (`"connected"`, `"connecting"`, or
/// anything else — anything other than the first two falls into Disconnected
/// so future values like `"error"` degrade gracefully).
pub fn status_label(auth: &AuthState, ws: &str) -> String {
    match auth {
        AuthState::Authenticated { hostname, .. } => match ws {
            "connected" => format!("● Connected — {}", hostname),
            "connecting" => format!("◌ Connecting — {}", hostname),
            _ => format!("⚠ Disconnected — {}", hostname),
        },
        AuthState::LocalOnly => "Not signed in — clipboard stays on this Mac".to_string(),
        AuthState::Authenticating { .. } => "Signing in…".to_string(),
        AuthState::ErrorRecoverable { .. } => "Sign-in error — open Dashboard".to_string(),
    }
}

/// Pure label producer for the "pending login requests" tray row.
/// Empty string when the row should be hidden (count == 0).
pub fn pending_label(count: usize) -> String {
    if count == 0 {
        String::new()
    } else if count == 1 {
        "1 pending login request".to_string()
    } else {
        format!("{} pending login requests", count)
    }
}

pub fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    // Disabled placeholder; first set_status call replaces this text.
    let status = MenuItem::with_id(app, "status", "…", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open Dashboard", true, None::<&str>)?;
    // Initially empty and disabled; set_pending_count enables it when codes arrive.
    let pending = MenuItem::with_id(app, "pending", "", false, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let check_updates = MenuItem::with_id(
        app,
        "check_updates",
        "Check for Updates…",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit Cinch", true, None::<&str>)?;

    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let sep4 = PredefinedMenuItem::separator(app)?;

    let menu = Menu::with_items(
        app,
        &[
            &status,
            &sep1,
            &open,
            &sep2,
            &pending,
            &sep3,
            &settings,
            &check_updates,
            &sep4,
            &quit,
        ],
    )?;

    let tray_img = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;
    let tray_icon = TrayIconBuilder::new()
        .icon(tray_img)
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("Cinch — Clipboard Sync")
        .on_menu_event(|app: &AppHandle, event| match event.id().as_ref() {
            "open" => crate::show_on_active_monitor(app),
            "pending" => {
                crate::show_on_active_monitor(app);
                use tauri_specta::Event as _;
                crate::events::TrayOpenPendingLogins.emit(app).ok();
            }
            "settings" => {
                crate::show_on_active_monitor(app);
                use tauri_specta::Event as _;
                crate::events::TrayOpenSettings.emit(app).ok();
            }
            "check_updates" => {
                let app2 = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::commands::updater::run_self_update_inner(app2).await {
                        log::warn!("tray check_updates failed: {}", e);
                    }
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    app.manage(TrayMenuItems {
        status,
        pending,
        tray: tray_icon,
    });

    info!("tray icon created");
    Ok(())
}

/// Refresh the tray's status row using the latest auth + ws values.
pub fn set_status(app: &AppHandle, auth: &AuthState, ws: &str) {
    if let Some(items) = app.try_state::<TrayMenuItems>() {
        let _ = items.status.set_text(status_label(auth, ws));
    }
}

/// Update the tray menu item to reflect pending device-code count.
/// Called from the WS handler when a `device_code_pending` frame arrives,
/// and from the TTL sweeper (Task 3.6) after expiry.
pub fn set_pending_count(app: &AppHandle, count: usize) {
    let label = pending_label(count);
    if let Some(items) = app.try_state::<TrayMenuItems>() {
        let _ = items.pending.set_text(&label);
        let _ = items.pending.set_enabled(count > 0);
    }
    // TODO(future): swap to a badged tray icon when count > 0.
    // Requires `icons/tray-badge.png` asset.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::state::{AuthErrorReason, AuthProgress, AuthState};

    #[test]
    fn pending_label_cases() {
        assert_eq!(pending_label(0), "");
        assert_eq!(pending_label(1), "1 pending login request");
        assert_eq!(pending_label(2), "2 pending login requests");
        assert_eq!(pending_label(5), "5 pending login requests");
    }

    fn auth_authenticated(hostname: &str) -> AuthState {
        AuthState::Authenticated {
            user_id: "u1".into(),
            device_id: "d1".into(),
            hostname: hostname.into(),
            relay_url: "https://relay.example".into(),
            active_relay_id: "r1".into(),
            machine_id: "m1".into(),
        }
    }

    #[test]
    fn status_label_authenticated_connected() {
        assert_eq!(
            status_label(&auth_authenticated("MacBook-Pro"), "connected"),
            "● Connected — MacBook-Pro"
        );
    }

    #[test]
    fn status_label_authenticated_connecting() {
        assert_eq!(
            status_label(&auth_authenticated("MacBook-Pro"), "connecting"),
            "◌ Connecting — MacBook-Pro"
        );
    }

    #[test]
    fn status_label_authenticated_disconnected() {
        assert_eq!(
            status_label(&auth_authenticated("MacBook-Pro"), "unconfigured"),
            "⚠ Disconnected — MacBook-Pro"
        );
        assert_eq!(
            status_label(&auth_authenticated("MacBook-Pro"), "error"),
            "⚠ Disconnected — MacBook-Pro"
        );
    }

    #[test]
    fn status_label_local_only() {
        assert_eq!(
            status_label(&AuthState::LocalOnly, "connected"),
            "Not signed in — clipboard stays on this Mac"
        );
        assert_eq!(
            status_label(&AuthState::LocalOnly, "unconfigured"),
            "Not signed in — clipboard stays on this Mac"
        );
    }

    #[test]
    fn status_label_authenticating() {
        let s = AuthState::Authenticating {
            progress: AuthProgress::SigningIn,
        };
        assert_eq!(status_label(&s, "connecting"), "Signing in…");
    }

    #[test]
    fn status_label_error_recoverable() {
        let s = AuthState::ErrorRecoverable {
            reason: AuthErrorReason::RelayUnreachable,
            retry_after_ms: None,
        };
        assert_eq!(
            status_label(&s, "connecting"),
            "Sign-in error — open Dashboard"
        );
    }
}
