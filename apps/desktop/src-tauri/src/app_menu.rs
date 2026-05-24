//! Custom application menu.
//!
//! macOS wires the default `PredefinedMenuItem::quit` to the native
//! `terminate:` selector (with the standard Cmd+Q key equivalent). tao installs
//! no `applicationShouldTerminate:` override, so `[NSApp terminate:]` kills the
//! process directly — it never produces a preventable `RunEvent::ExitRequested`,
//! so the `.run` handler's `prevent_exit()` can't intercept Cmd+Q.
//!
//! To make Cmd+Q *hide* the window and keep the menu-bar agent alive, we own the
//! menu: the Cmd+Q slot is a plain `MenuItem` whose activation fires
//! `on_menu_event` (handled in [`handle_menu_event`]) instead of `terminate:`.
//! The real quit stays on the tray's "Quit Cinch" item (`app.exit(0)`).
//!
//! Replacing Tauri's default menu drops its built-in Edit/Window shortcuts, so
//! we re-supply them here; otherwise Cmd+C/V/X/A and Cmd+W/Cmd+M would stop
//! working in the dashboard. For an Accessory app the menu bar is not drawn, but
//! its key equivalents are still processed while a Cinch window is focused —
//! the same mechanism the Edit shortcuts rely on.

use tauri::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, Wry};

/// Menu id for the Cmd+Q "hide window" item, matched in [`handle_menu_event`].
pub const HIDE_WINDOW_ID: &str = "app_hide_window";

/// Build the application menu. See the module docs for why this replaces the
/// Tauri default rather than extending it.
pub fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    // App submenu (the first submenu is the macOS application menu). The Cmd+Q
    // item is a plain MenuItem — NOT the predefined quit — so Cmd+Q fires a menu
    // event we turn into a window hide instead of `terminate:`.
    // Labeled "Hide Window" because it calls window.hide() (the app stays
    // alive); the real quit is the tray's "Quit Cinch". Standard app-menu items
    // like Hide Others / Show All / Quit are intentionally omitted — they are
    // never visible for an Accessory app and not needed for a menu-bar agent.
    let hide_window = MenuItem::with_id(
        app,
        HIDE_WINDOW_ID,
        "Hide Window",
        true,
        Some("CmdOrCtrl+Q"),
    )?;
    let app_menu = Submenu::with_items(
        app,
        "Cinch",
        true,
        &[
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &hide_window,
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

    // Window submenu — preserves Cmd+M (minimize) and Cmd+W (close → hide via
    // the CloseRequested handler in lib.rs).
    let window_menu = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;

    Menu::with_items(app, &[&app_menu, &edit_menu, &window_menu])
}

/// Handle application-menu events. Cmd+Q (our custom item) hides every webview
/// window so the app keeps running in the menu bar; the real quit is the tray's
/// "Quit Cinch".
pub fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    if event.id().as_ref() == HIDE_WINDOW_ID {
        for window in app.webview_windows().values() {
            let _ = window.hide();
        }
    }
}
