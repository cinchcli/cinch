//! Main-window placement, focus stealing, and global-shortcut registration.
//!
//! Pulled out of `lib.rs` because these helpers are tightly coupled to
//! macOS NSWindow / NSRunningApplication APIs and are easier to reason
//! about as a self-contained module.

use std::sync::Arc;

use log::info;
use tauri::Manager;

use crate::commands;
use crate::store;
#[cfg(target_os = "macos")]
use crate::PreviousAppPid;

/// Show the main window centered on the monitor that currently has the mouse cursor.
/// Falls back to simple show+focus if cursor or monitor data is unavailable.
pub(crate) fn show_on_active_monitor(app: &tauri::AppHandle) {
    // Capture the frontmost app before Cinch steals focus, so we can restore it on copy.
    #[cfg(target_os = "macos")]
    capture_frontmost_app_pid(app);

    let Some(window) = app.get_webview_window("main") else {
        return;
    };

    let result = (|| -> tauri::Result<()> {
        let cursor = app.cursor_position()?;
        let monitors = app.available_monitors()?;
        let boxes: Vec<crate::window_snap::MonitorBox> =
            monitors.iter().map(commands::window::to_box).collect();

        let s = window
            .outer_size()
            .unwrap_or(tauri::PhysicalSize::new(960, 600));
        let win = crate::window_snap::WinSize {
            w: s.width as i32,
            h: s.height as i32,
        };

        let saved = app
            .try_state::<Arc<store::db::Database>>()
            .and_then(|db| commands::window::load_placement(&db));

        // Always reposition: choose_placement restores the saved per-monitor
        // placement, else anchors on the cursor/first monitor, bottoming out
        // at (0,0) only when no monitors are reported (degenerate/headless).
        let (x, y) =
            crate::window_snap::choose_placement(saved.as_ref(), &boxes, (cursor.x, cursor.y), win);
        window.set_position(tauri::PhysicalPosition::new(x, y))?;
        Ok(())
    })();

    if let Err(e) = result {
        log::warn!("show_on_active_monitor: could not reposition window: {}", e);
    }

    let _ = window.show();
    // Promote the whole app above other apps before focusing the window —
    // `set_focus` alone only reorders within the active app on macOS.
    #[cfg(target_os = "macos")]
    activate_self();
    let _ = window.set_focus();
}

/// Captures the pid of the macOS frontmost application and stores it in PreviousAppPid state.
#[cfg(target_os = "macos")]
fn capture_frontmost_app_pid(app: &tauri::AppHandle) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    let pid: i32 = unsafe {
        let workspace: *mut Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let frontmost: *mut Object = msg_send![workspace, frontmostApplication];
        if frontmost.is_null() {
            return;
        }
        msg_send![frontmost, processIdentifier]
    };

    if let Some(state) = app.try_state::<PreviousAppPid>() {
        if let Ok(mut guard) = state.lock() {
            *guard = Some(pid);
        }
    }
}

/// Activates a macOS application by its process identifier.
#[cfg(target_os = "macos")]
pub(crate) fn activate_app_by_pid(pid: i32) {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let app: *mut Object =
            msg_send![class!(NSRunningApplication), runningApplicationWithProcessIdentifier: pid];
        if app.is_null() {
            return;
        }
        // NSApplicationActivateIgnoringOtherApps = 2
        let _: bool = msg_send![app, activateWithOptions: 2u64];
    }
}

/// Brings the current process to the front on macOS.
///
/// `NSWindow.makeKeyAndOrderFront:` (what Tauri's `set_focus` calls) only reorders
/// windows *within* the active application. If another app is frontmost when the
/// global shortcut fires, the Cinch window appears layered between that app's
/// windows instead of on top of everything. Activating the running application
/// itself promotes Cinch above all other apps in the global window order.
#[cfg(target_os = "macos")]
fn activate_self() {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let app: *mut Object = msg_send![class!(NSRunningApplication), currentApplication];
        if app.is_null() {
            return;
        }
        // NSApplicationActivateIgnoringOtherApps = 2
        let _: bool = msg_send![app, activateWithOptions: 2u64];
    }
}

/// Configure the NSWindow so that external window managers (Rectangle, Moom, etc.) can
/// move it via the Accessibility API.
///
/// `decorations: false` produces NSWindowStyleMaskBorderless, whose macOS default is
/// `isMovable = false`. Rectangle calls `AXUIElementSetAttributeValue(kAXPositionAttribute)`
/// which silently no-ops when `isMovable` is false. Setting it to true fixes "Move to
/// Next/Previous Display" while leaving mouse drag behavior unchanged.
///
/// NSWindowCollectionBehaviorManaged (bit 2) makes the window appear in Mission Control
/// and participate in Spaces, which some window managers require before they will manage it.
#[cfg(target_os = "macos")]
pub(crate) fn configure_macos_window(app: &tauri::AppHandle) {
    use objc::runtime::{Object, YES};
    use objc::{msg_send, sel, sel_impl};

    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(ns_window_ptr) = window.ns_window() else {
        return;
    };
    unsafe {
        let ns_window = ns_window_ptr as *mut Object;
        // Allow AX-based moves (fixes Rectangle "Move to Next/Prev Display")
        let _: () = msg_send![ns_window, setMovable: YES];
        // NSWindowCollectionBehaviorManaged=4, NSWindowCollectionBehaviorParticipatesInCycle=32
        let behavior: u64 = (1 << 2) | (1 << 5);
        let _: () = msg_send![ns_window, setCollectionBehavior: behavior];
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn configure_macos_window(_app: &tauri::AppHandle) {}

/// Make the app a background menu-bar agent on macOS: no Dock icon, hidden
/// from the Cmd+Tab app switcher, and no top-left app menu. Only the tray
/// status icon remains.
///
/// The app's `NSApp.mainMenu` is not drawn for an Accessory app, but its key
/// equivalents still fire while a window is focused. The custom menu in
/// `app_menu.rs` relies on this so standard shortcuts keep working (Edit
/// shortcuts like Cmd+C/V/X/A, and Window shortcuts like Cmd+W/Cmd+M).
#[cfg(target_os = "macos")]
pub(crate) fn configure_activation_policy(app: &tauri::AppHandle) {
    if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Accessory) {
        log::warn!("failed to set Accessory activation policy: {}", e);
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn configure_activation_policy(_app: &tauri::AppHandle) {}

/// Register the opt-in "send current clipboard" shortcut. No-op when the user
/// has not configured one (the send hotkey is opt-in).
pub(crate) fn register_send_shortcut(app: &tauri::AppHandle) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    let Some(shortcut_str) = app.try_state::<crate::SharedStore>().and_then(|store| {
        client_core::store::settings::send_shortcut(&store)
            .ok()
            .flatten()
    }) else {
        return; // unset (or store not yet managed) → disabled (opt-in)
    };

    let handle = app.clone();
    if let Err(e) =
        app.global_shortcut()
            .on_shortcut(shortcut_str.as_str(), move |_app, _shortcut, event| {
                if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                    let h = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        // Resolve owned Arcs from the handle — avoids holding Tauri
                        // `State` (a borrow of `h`) across the await point.
                        let clipboard = h
                            .state::<Arc<crate::clipboard::ClipboardService>>()
                            .inner()
                            .clone();
                        let pusher = h
                            .state::<crate::app_state::LocalPusherHandle>()
                            .inner()
                            .clone();
                        if let Err(e) = crate::commands::clips::send_current_clipboard_impl(
                            &clipboard, &pusher, &h,
                        )
                        .await
                        {
                            log::warn!("send shortcut: {}", e);
                        }
                    });
                }
            })
    {
        log::warn!("failed to register send shortcut {}: {}", shortcut_str, e);
    }
}

pub(crate) fn register_global_shortcuts(app: &tauri::AppHandle) {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;

    // Read persisted shortcut preference, fall back to default (D-08)
    let shortcut_str =
        client_core::store::settings::global_shortcut(&app.state::<crate::SharedStore>())
            .ok()
            .flatten()
            .unwrap_or_else(|| "CmdOrCtrl+Shift+W".to_string());

    let handle = app.clone();
    if let Err(e) =
        app.global_shortcut()
            .on_shortcut(shortcut_str.as_str(), move |_app, shortcut, event| {
                if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                    info!("global shortcut pressed: {}", shortcut);
                    show_on_active_monitor(&handle);
                }
            })
    {
        log::warn!(
            "failed to register {} shortcut: {} (may conflict with another app)",
            shortcut_str,
            e
        );
    }
}
