use log::info;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};

use crate::auth::state::AuthState;

pub struct TrayMenuItems {
    pub status: MenuItem<tauri::Wry>,
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

pub fn setup_tray(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    // Disabled placeholder; first set_status call replaces this text.
    let status = MenuItem::with_id(app, "status", "…", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open Dashboard", true, None::<&str>)?;
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

    let menu = Menu::with_items(
        app,
        &[
            &status,
            &sep1,
            &open,
            &sep2,
            &settings,
            &check_updates,
            &sep3,
            &quit,
        ],
    )?;

    let tray_img = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon.png"))?;
    // Building the tray registers it into Tauri's resource table (an
    // independent strong reference held for the AppHandle's lifetime), so the
    // icon survives even though we don't retain the handle here.
    let _tray_icon = TrayIconBuilder::new()
        .icon(tray_img)
        .icon_as_template(true)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("Cinch — Clipboard Sync")
        .on_menu_event(|app: &AppHandle, event| {
            use tauri_specta::Event as _;
            match event.id().as_ref() {
                "open" => crate::show_on_active_monitor(app),
                "settings" => {
                    crate::show_on_active_monitor(app);
                    crate::events::TrayOpenSettings.emit(app).ok();
                }
                "check_updates" => {
                    let app2 = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = crate::commands::updater::run_self_update_inner(app2).await
                        {
                            log::warn!("tray check_updates failed: {}", e);
                        }
                    });
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .build(app)?;

    app.manage(TrayMenuItems { status });

    info!("tray icon created");
    Ok(())
}

/// Refresh the tray's status row using the latest auth + ws values.
pub fn set_status(app: &AppHandle, auth: &AuthState, ws: &str) {
    if let Some(items) = app.try_state::<TrayMenuItems>() {
        let _ = items.status.set_text(status_label(auth, ws));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::state::{AuthErrorReason, AuthProgress, AuthState};

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
