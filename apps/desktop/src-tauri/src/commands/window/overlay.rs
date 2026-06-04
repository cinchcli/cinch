//! The transparent snap-guide overlay window: its drag-session flag,
//! lifecycle, placement, and the per-frame guide emission.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};
use tauri_specta::Event;

use super::geometry::win_size;
use crate::events::SnapGuideUpdate;
use crate::window_snap::{anchor_for, MonitorBox, WinSize, SNAP_THRESHOLD_PX};

pub const OVERLAY_LABEL: &str = "snap-overlay";

/// Drag-session flag. `true` between `snap_drag_start` and drag-end.
pub struct SnapState {
    pub active: Arc<Mutex<bool>>,
}

impl SnapState {
    pub fn new() -> Self {
        Self {
            active: Arc::new(Mutex::new(false)),
        }
    }
}

/// Create the overlay window once (hidden). Idempotent.
pub fn ensure_overlay(app: &AppHandle) {
    if app.get_webview_window(OVERLAY_LABEL).is_some() {
        return;
    }
    let res = WebviewWindowBuilder::new(
        app,
        OVERLAY_LABEL,
        WebviewUrl::App("index.html?overlay=1".into()),
    )
    .title("")
    .transparent(true)
    .decorations(false)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .visible(false)
    .accept_first_mouse(false)
    .build();

    match res {
        Ok(w) => {
            let _ = w.set_ignore_cursor_events(true);
        }
        Err(e) => log::warn!("snap overlay create failed (non-fatal): {e}"),
    }
}

/// Position+size the overlay to exactly cover `m`, show it, and push an
/// initial guide frame.
pub(crate) fn place_overlay(app: &AppHandle, m: &MonitorBox, snap_info: (bool, bool, f64, f64)) {
    let Some(overlay) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    let _ = overlay.set_position(PhysicalPosition::new(m.x, m.y));
    let _ = overlay.set_size(PhysicalSize::new(m.w as u32, m.h as u32));
    let _ = overlay.show();
    let _ = overlay.set_ignore_cursor_events(true);
    emit_guide(app, m, snap_info, true);
}

/// Emit one guide frame for monitor `m`.
pub(crate) fn emit_guide(
    app: &AppHandle,
    m: &MonitorBox,
    snap_info: (bool, bool, f64, f64),
    visible: bool,
) {
    let win = win_size(app);
    let (ax, ay) = anchor_for(m, win);
    let _ = SnapGuideUpdate {
        monitor_x: m.x,
        monitor_y: m.y,
        monitor_w: m.w as u32,
        monitor_h: m.h as u32,
        scale: m.scale,
        anchor_x: ax,
        anchor_y: ay,
        win_w: win.w as u32,
        win_h: win.h as u32,
        snap_x: snap_info.0,
        snap_y: snap_info.1,
        dist_x: snap_info.2,
        dist_y: snap_info.3,
        visible,
    }
    .emit(app);
}

/// Is the panel center currently within the snap threshold of `m`'s anchor on each axis?
/// Returns (snap_x, snap_y, dist_x, dist_y) where distances are logical pixels.
pub(crate) fn within_snap(app: &AppHandle, m: &MonitorBox) -> (bool, bool, f64, f64) {
    let Some(win) = app.get_webview_window("main") else {
        return (false, false, 0.0, 0.0);
    };
    let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) else {
        return (false, false, 0.0, 0.0);
    };
    let w = WinSize {
        w: size.width as i32,
        h: size.height as i32,
    };
    let center = (pos.x + w.w / 2, pos.y + w.h / 2);
    let (ax, ay) = anchor_for(m, w);
    let anchor_center = (ax + w.w / 2, ay + w.h / 2);

    let s = m.scale.max(1.0);
    let dx_phys = (center.0 - anchor_center.0).abs() as f64;
    let dy_phys = (center.1 - anchor_center.1).abs() as f64;

    let dx = dx_phys / s;
    let dy = dy_phys / s;

    (
        dx_phys <= SNAP_THRESHOLD_PX,
        dy_phys <= SNAP_THRESHOLD_PX,
        dx,
        dy,
    )
}
