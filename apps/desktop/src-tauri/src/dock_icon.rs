//! macOS Dock-icon theming.
//!
//! The bundle ships one dark icon (cream mark on an ink squircle). While a
//! window is on screen the app is a Regular app (see
//! `window_manage::set_dock_visible`) and a Dock icon appears; this module
//! makes that Dock icon follow the system appearance via
//! `[NSApp setApplicationIconImage:]`.
//!
//! Concerns are split: this module owns the *image* (which variant + reacting
//! to `ThemeChanged`); `window_manage` owns *presence* (Regular vs Accessory).
//! `applicationIconImage` persists across activation-policy changes, so
//! staging it at startup means no default-icon flash when the policy first
//! flips to Regular.

use tauri::{Manager, Theme};

/// Dark default (cream mark on ink squircle) — also the bundle icon.
const ICON_DARK: &[u8] = include_bytes!("../icons/icon.png");
/// Light variant (ink mark on cream squircle).
const ICON_LIGHT: &[u8] = include_bytes!("../icons/icon-light.png");

/// Pick the Dock-icon bytes for a system appearance. Pure; unit-tested.
pub(crate) fn icon_bytes_for(theme: Theme) -> &'static [u8] {
    match theme {
        Theme::Light => ICON_LIGHT,
        // Dark, and any future/unknown (`Theme` is non-exhaustive) -> dark.
        _ => ICON_DARK,
    }
}

/// Read the current system appearance from the main window, defaulting to
/// Dark if the window or its theme is unavailable.
pub(crate) fn current_theme(app: &tauri::AppHandle) -> Theme {
    app.get_webview_window("main")
        .and_then(|w| w.theme().ok())
        .unwrap_or(Theme::Dark)
}

/// Set the macOS Dock icon to the variant matching `theme`. No-op off macOS.
pub(crate) fn apply_dock_icon(app: &tauri::AppHandle, theme: Theme) {
    #[cfg(target_os = "macos")]
    {
        // `&'static [u8]` is Send; AppKit pointers are not, so the main-thread
        // closure captures only the bytes and re-fetches NSApp itself.
        let bytes = icon_bytes_for(theme);
        let _ = app.run_on_main_thread(move || set_ns_app_icon(bytes));
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, theme);
    }
}

#[cfg(target_os = "macos")]
fn set_ns_app_icon(png: &'static [u8]) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let data: *mut Object =
            msg_send![class!(NSData), dataWithBytes: png.as_ptr() length: png.len()];
        if data.is_null() {
            return;
        }
        let image: *mut Object = msg_send![class!(NSImage), alloc];
        let image: *mut Object = msg_send![image, initWithData: data];
        if image.is_null() {
            return;
        }
        let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        let _: () = msg_send![ns_app, setApplicationIconImage: image];
    }
}

/// Stage the initial themed Dock icon and install a `ThemeChanged` listener on
/// the main window so the icon live-swaps when the user toggles appearance.
pub(crate) fn setup(app: &tauri::AppHandle) {
    apply_dock_icon(app, current_theme(app));

    if let Some(win) = app.get_webview_window("main") {
        let handle = app.clone();
        win.on_window_event(move |event| {
            if let tauri::WindowEvent::ThemeChanged(theme) = event {
                apply_dock_icon(&handle, *theme);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_and_dark_differ() {
        let light = icon_bytes_for(Theme::Light);
        let dark = icon_bytes_for(Theme::Dark);
        assert!(!light.is_empty(), "light icon embedded");
        assert!(!dark.is_empty(), "dark icon embedded");
        assert_ne!(light, dark, "light and dark icons must differ");
    }
}
