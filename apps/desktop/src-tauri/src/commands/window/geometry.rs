//! Monitor geometry helpers: converting Tauri monitors to the pure
//! [`MonitorBox`] and resolving the panel's current / cursor monitor.

use crate::window_snap::{MonitorBox, WinSize};
use tauri::{AppHandle, Manager};

/// Convert a Tauri monitor to the pure `MonitorBox`.
pub(crate) fn to_box(m: &tauri::Monitor) -> MonitorBox {
    let p = m.position();
    let s = m.size();
    MonitorBox {
        x: p.x,
        y: p.y,
        w: s.width as i32,
        h: s.height as i32,
        name: m.name().map(|n| n.to_string()),
        scale: m.scale_factor(),
    }
}

pub(crate) fn win_size(app: &AppHandle) -> WinSize {
    app.get_webview_window("main")
        .and_then(|w| w.outer_size().ok())
        .map(|s| WinSize {
            w: s.width as i32,
            h: s.height as i32,
        })
        .unwrap_or(WinSize { w: 960, h: 600 })
}

/// The monitor whose rectangle contains the panel's current center, falling
/// back to the cursor monitor, then the primary monitor.
pub(crate) fn current_monitor(app: &AppHandle) -> Option<MonitorBox> {
    let win = app.get_webview_window("main")?;
    let monitors = app.available_monitors().ok()?;

    let contains = |m: &tauri::Monitor, x: f64, y: f64| {
        let p = m.position();
        let s = m.size();
        x >= p.x as f64
            && x < (p.x + s.width as i32) as f64
            && y >= p.y as f64
            && y < (p.y + s.height as i32) as f64
    };

    // 1. The monitor containing the panel's center.
    if let (Ok(pos), Ok(size)) = (win.outer_position(), win.outer_size()) {
        let cx = pos.x as f64 + size.width as f64 / 2.0;
        let cy = pos.y as f64 + size.height as f64 / 2.0;
        if let Some(m) = monitors.iter().find(|m| contains(m, cx, cy)) {
            return Some(to_box(m));
        }
    }
    // 2. The monitor under the cursor (the "active" monitor) — used when the
    //    panel center is transiently off all monitors mid cross-monitor drag,
    //    consistent with choose_placement's cursor-monitor preference.
    if let Ok(cur) = app.cursor_position() {
        if let Some(m) = monitors.iter().find(|m| contains(m, cur.x, cur.y)) {
            return Some(to_box(m));
        }
    }
    // 3. Primary monitor as the last resort.
    app.primary_monitor().ok().flatten().map(|m| to_box(&m))
}

pub(crate) fn cursor_monitor(app: &AppHandle) -> Option<MonitorBox> {
    let monitors = app.available_monitors().ok()?;
    let contains = |m: &tauri::Monitor, x: f64, y: f64| {
        let p = m.position();
        let s = m.size();
        x >= p.x as f64
            && x < (p.x + s.width as i32) as f64
            && y >= p.y as f64
            && y < (p.y + s.height as i32) as f64
    };

    if let Ok(cur) = app.cursor_position() {
        if let Some(m) = monitors.iter().find(|m| contains(m, cur.x, cur.y)) {
            return Some(to_box(m));
        }
    }

    app.primary_monitor().ok().flatten().map(|m| to_box(&m))
}
