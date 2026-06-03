//! Custom application menu.
//!
//! Replacing Tauri's default menu drops its built-in Edit/Window shortcuts, so
//! we re-supply them here; otherwise Cmd+C/V/X/A (Edit) and Cmd+W/Cmd+M (Window)
//! stop working in the dashboard.
//!
//! On macOS the app runs as an Accessory (menu-bar agent): the menu bar is not
//! drawn, but key equivalents are still processed while a Cinch window is
//! focused — the same mechanism the Edit shortcuts rely on.
//!
//! Behavior we want:
//! - Cmd+Q quits the app (standard macOS behavior)
//! - Cmd+W closes the Dashboard window (implemented as hide, so it can be shown
//!   again from the tray)

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, Wry};

/// Menu id for the Cmd+W "close dashboard" item, matched in [`handle_menu_event`].
pub const CLOSE_DASHBOARD_ID: &str = "window_close_dashboard";

/// Build the application menu. See the module docs for why this replaces the
/// Tauri default rather than extending it.
pub fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    // App submenu (the first submenu is the macOS application menu).
    // Even though the menu bar is not drawn for an Accessory app, key
    // equivalents still fire while a window is focused.
    let app_menu = Submenu::with_items(
        app,
        "Cinch",
        true,
        &[
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;

    // Edit submenu — required so text-field shortcuts keep working once we
    // replace the default menu.
    let edit_menu = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;

    // Window submenu — preserves Cmd+M (minimize) and implements Cmd+W as
    // "close dashboard" (hide the main window so it can be restored from tray).
    let close_dashboard = MenuItem::with_id(
        app,
        CLOSE_DASHBOARD_ID,
        "Close Window",
        true,
        Some("CmdOrCtrl+W"),
    )?;
    let window_menu = Submenu::with_items(
        app,
        "Window",
        true,
        &[&PredefinedMenuItem::minimize(app, None)?, &close_dashboard],
    )?;

    Menu::with_items(app, &[&app_menu, &edit_menu, &window_menu])
}

/// Handle application-menu events.
///
/// Cmd+W closes the Dashboard window (implemented as hide).
pub fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    if event.id().as_ref() == CLOSE_DASHBOARD_ID {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.hide();
            crate::window_manage::set_dock_visible(app, false);
        }
    }
}
