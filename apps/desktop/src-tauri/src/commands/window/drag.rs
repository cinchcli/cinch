//! Drag lifecycle: starting a snap session, tracking the panel across
//! monitors while it moves, and resolving + persisting the drop placement.

use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, PhysicalPosition};

use super::geometry::current_monitor;
use super::overlay::{emit_guide, place_overlay, within_snap, SnapState, OVERLAY_LABEL};
use crate::window_snap::{
    anchor_for, monitor_fingerprint, resolve_drop, Placement, WinSize, SNAP_THRESHOLD_PX,
};

/// True while the left mouse button is physically held down (macOS).
#[cfg(target_os = "macos")]
fn left_mouse_down() -> bool {
    use objc::{class, msg_send, sel, sel_impl};
    let buttons: usize = unsafe { msg_send![class!(NSEvent), pressedMouseButtons] };
    buttons & 1 != 0
}

#[cfg(not(target_os = "macos"))]
fn left_mouse_down() -> bool {
    false
}

/// Frontend calls this on the same mousedown that starts the native drag.
/// Shows the overlay on the current monitor and spawns a drag-end poll.
#[tauri::command]
#[specta::specta]
pub fn snap_drag_start(app: AppHandle, state: tauri::State<'_, SnapState>) -> Result<(), String> {
    {
        let mut active = state.active.lock().map_err(|e| e.to_string())?;
        if *active {
            return Ok(()); // already dragging
        }
        *active = true;
    }

    let Some(m) = current_monitor(&app) else {
        // No monitor data → degrade gracefully: no overlay, no snap.
        if let Ok(mut a) = state.active.lock() {
            *a = false;
        }
        return Ok(());
    };
    place_overlay(&app, &m, within_snap(&app, &m));

    let active = state.active.clone();
    let app_poll = app.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        // Wait for the button to actually be down (gesture in flight),
        // then for release. 30s hard cap is the missed-mouse-up backstop.
        loop {
            if started.elapsed() > Duration::from_secs(30) {
                break;
            }
            if !left_mouse_down() && started.elapsed() > Duration::from_millis(120) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        if let Ok(mut a) = active.lock() {
            *a = false;
        }
        let app_for_drag = app_poll.clone();
        let _ = app_poll.run_on_main_thread(move || {
            finish_drag(&app_for_drag);
        });
    });

    Ok(())
}

/// Called for every `WindowEvent::Moved` on the main window. No-op unless a
/// drag session is active. Re-targets the overlay if the panel crossed to
/// another monitor and streams a fresh guide frame.
pub fn on_window_moved(app: &AppHandle) {
    let Some(state) = app.try_state::<SnapState>() else {
        return;
    };
    let is_active = state.active.lock().map(|g| *g).unwrap_or(false);
    if !is_active {
        return;
    }
    let Some(m) = current_monitor(app) else {
        return;
    };
    if let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) {
        // Re-cover the monitor the panel is now on (handles cross-monitor drag).
        let needs_move = overlay
            .outer_position()
            .map(|p| p.x != m.x || p.y != m.y)
            .unwrap_or(true)
            || overlay
                .outer_size()
                .map(|s| s.width != m.w as u32 || s.height != m.h as u32)
                .unwrap_or(true);
        if needs_move {
            let _ = overlay.set_position(PhysicalPosition::new(m.x, m.y));
            let _ = overlay.set_size(tauri::PhysicalSize::new(m.w as u32, m.h as u32));
        }
        if !overlay.is_visible().unwrap_or(false) {
            let _ = overlay.show();
        }
    }
    emit_guide(app, &m, within_snap(app, &m), true);
}

pub fn save_placement(store: &crate::SharedStore, p: &Placement) {
    match serde_json::to_string(p) {
        Ok(json) => {
            if let Err(e) = client_core::store::settings::set_window_placement(store, &json) {
                log::warn!("save_placement failed (non-fatal): {e}");
            }
        }
        Err(e) => log::warn!("save_placement serialize failed: {e}"),
    }
}

pub fn load_placement(store: &crate::SharedStore) -> Option<Placement> {
    let raw = client_core::store::settings::window_placement(store)
        .ok()
        .flatten()?;
    serde_json::from_str(&raw).ok()
}

/// Called once on the main thread when the drag ends. Snaps the panel to the
/// anchor if released within threshold (instant, no animation), otherwise
/// leaves it where dropped; persists the placement; hides the overlay.
pub fn finish_drag(app: &AppHandle) {
    if let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = overlay.hide();
    }
    let Some(win) = app.get_webview_window("main") else {
        return;
    };
    let Some(m) = current_monitor(app) else {
        return;
    };
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
        return;
    };
    let w = WinSize {
        w: size.width as i32,
        h: size.height as i32,
    };
    let center = (pos.x + w.w / 2, pos.y + w.h / 2);
    let (ax, ay) = anchor_for(&m, w);
    let anchor_center = (ax + w.w / 2, ay + w.h / 2);

    let ((nx, ny), anchored) = resolve_drop(center, anchor_center, w, SNAP_THRESHOLD_PX);
    // Snap → reposition to the anchor (instant, no animation). Free drop →
    // leave the window exactly where released; (nx, ny) already equals its
    // current top-left, and it is persisted below either way.
    if anchored {
        let _ = win.set_position(PhysicalPosition::new(nx, ny)); // instant jump
    }

    if let Some(store) = app.try_state::<crate::SharedStore>() {
        save_placement(
            &store,
            &Placement {
                monitor: monitor_fingerprint(&m),
                x: nx,
                y: ny,
                anchored,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::window_snap::Placement;

    fn test_store() -> crate::SharedStore {
        Arc::new(client_core::store::Store::open(std::path::Path::new(":memory:")).unwrap())
    }

    #[test]
    fn placement_persists_through_settings_store() {
        let store = test_store();
        assert!(super::load_placement(&store).is_none());

        let p = Placement {
            monitor: "name:A".into(),
            x: 5,
            y: 6,
            anchored: true,
        };
        super::save_placement(&store, &p);

        let got = super::load_placement(&store).expect("saved placement");
        assert_eq!(got, p);

        // sanity: stored under the documented key as JSON, via client-core settings
        let raw = client_core::store::settings::window_placement(&store)
            .unwrap()
            .unwrap();
        assert!(raw.contains("\"monitor\":\"name:A\""));
    }
}
