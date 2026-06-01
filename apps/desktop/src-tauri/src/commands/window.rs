//! Window snap-guide orchestration: a transparent click-through overlay
//! window shows a full-screen "H" while the user drags the panel, and the
//! panel snaps to a single per-monitor anchor (or stays free) on release.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Size, WebviewUrl, WebviewWindowBuilder,
};
use tauri_specta::Event;

use crate::events::{CopyToastRequested, SnapGuideUpdate};
use crate::window_snap::{
    anchor_for, monitor_fingerprint, resolve_drop, MonitorBox, Placement, WinSize,
    SNAP_THRESHOLD_PX,
};

pub const OVERLAY_LABEL: &str = "snap-overlay";
pub const COPY_TOAST_LABEL: &str = "copy-toast";
const COPY_TOAST_REFERENCE_WIDTH_PX: f64 = 1920.0;
const COPY_TOAST_REFERENCE_HEIGHT_PX: f64 = 1080.0;
const COPY_TOAST_MAX_WIDTH_PX: u32 = 438;
const COPY_TOAST_MAX_HEIGHT_PX: u32 = 80;
const COPY_TOAST_WIDTH_RATIO: f64 = 438.0 / COPY_TOAST_REFERENCE_WIDTH_PX;
const COPY_TOAST_HEIGHT_RATIO: f64 = 80.0 / COPY_TOAST_REFERENCE_HEIGHT_PX;
const COPY_TOAST_BOTTOM_OFFSET_RATIO: f64 = 120.0 / COPY_TOAST_REFERENCE_HEIGHT_PX;
const COPY_TOAST_DURATION_MS: u64 = 1800;

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

/// Monotonic token for copy-toast hide timers. A newer toast must not be hidden
/// by an older timer that is still sleeping.
pub struct CopyToastState {
    generation: Arc<Mutex<u64>>,
}

impl CopyToastState {
    pub fn new() -> Self {
        Self {
            generation: Arc::new(Mutex::new(0)),
        }
    }

    fn next_generation(&self) -> Result<u64, String> {
        let mut generation = self.generation.lock().map_err(|e| e.to_string())?;
        *generation = generation.saturating_add(1);
        Ok(*generation)
    }
}

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

fn win_size(app: &AppHandle) -> WinSize {
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
fn current_monitor(app: &AppHandle) -> Option<MonitorBox> {
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

fn cursor_monitor(app: &AppHandle) -> Option<MonitorBox> {
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

fn copy_toast_metrics(m: &MonitorBox) -> (u32, u32, i32) {
    let width = ((m.w as f64 * COPY_TOAST_WIDTH_RATIO).round().max(1.0) as u32)
        .min(COPY_TOAST_MAX_WIDTH_PX);
    let height = ((m.h as f64 * COPY_TOAST_HEIGHT_RATIO).round().max(1.0) as u32)
        .min(COPY_TOAST_MAX_HEIGHT_PX);
    let bottom_offset = (m.h as f64 * COPY_TOAST_BOTTOM_OFFSET_RATIO).round() as i32;
    (width, height, bottom_offset)
}

fn reference_copy_toast_metrics() -> (u32, u32, i32) {
    copy_toast_metrics(&MonitorBox {
        x: 0,
        y: 0,
        w: COPY_TOAST_REFERENCE_WIDTH_PX as i32,
        h: COPY_TOAST_REFERENCE_HEIGHT_PX as i32,
        name: None,
        scale: 1.0,
    })
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

/// Create the detached copy-toast window once (hidden). The window renders a
/// compact Raycast-style confirmation and ignores cursor input.
pub fn ensure_copy_toast(app: &AppHandle) {
    if app.get_webview_window(COPY_TOAST_LABEL).is_some() {
        return;
    }
    let (width, height, _) = cursor_monitor(app)
        .map(|m| copy_toast_metrics(&m))
        .unwrap_or_else(reference_copy_toast_metrics);
    let res = WebviewWindowBuilder::new(
        app,
        COPY_TOAST_LABEL,
        WebviewUrl::App("index.html?copy-toast=1".into()),
    )
    .title("")
    .inner_size(width as f64, height as f64)
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
        Err(e) => log::warn!("copy toast window create failed (non-fatal): {e}"),
    }
}

fn place_copy_toast(app: &AppHandle) {
    let Some(window) = app.get_webview_window(COPY_TOAST_LABEL) else {
        return;
    };
    let Some(m) = cursor_monitor(app) else {
        return;
    };
    let (width, height, bottom_offset) = copy_toast_metrics(&m);
    let width_i32 = width as i32;
    let height_i32 = height as i32;
    let x = m.x + ((m.w - width_i32) / 2).max(0);
    let y = m.y + m.h - height_i32 - bottom_offset;

    let _ = window.set_size(Size::Physical(PhysicalSize::new(width, height)));
    let _ = window.set_position(PhysicalPosition::new(x, y));
    let _ = window.set_ignore_cursor_events(true);
}

#[tauri::command]
#[specta::specta]
pub fn show_copy_toast(
    app: AppHandle,
    state: tauri::State<'_, CopyToastState>,
    message: String,
) -> Result<(), String> {
    ensure_copy_toast(&app);
    place_copy_toast(&app);

    let generation = state.next_generation()?;
    let Some(window) = app.get_webview_window(COPY_TOAST_LABEL) else {
        return Ok(());
    };
    let message = if message.trim().is_empty() {
        "Copied to clipboard".to_string()
    } else {
        message
    };
    let _ = window.show();
    let _ = window.set_ignore_cursor_events(true);
    let _ = CopyToastRequested {
        message,
        duration_ms: COPY_TOAST_DURATION_MS,
    }
    .emit(&app);

    let app_for_hide = app.clone();
    let state_for_hide = state.generation.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(COPY_TOAST_DURATION_MS));
        let still_current = state_for_hide
            .lock()
            .map(|current| *current == generation)
            .unwrap_or(false);
        if !still_current {
            return;
        }
        let app_for_main = app_for_hide.clone();
        let _ = app_for_hide.run_on_main_thread(move || {
            if let Some(window) = app_for_main.get_webview_window(COPY_TOAST_LABEL) {
                let _ = window.hide();
            }
        });
    });

    Ok(())
}

/// Position+size the overlay to exactly cover `m`, show it, and push an
/// initial guide frame.
fn place_overlay(app: &AppHandle, m: &MonitorBox, snap_info: (bool, bool, f64, f64)) {
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
pub fn emit_guide(
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
pub fn within_snap(app: &AppHandle, m: &MonitorBox) -> (bool, bool, f64, f64) {
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
            crate::commands::window::finish_drag(&app_for_drag);
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
            let _ = overlay.set_size(PhysicalSize::new(m.w as u32, m.h as u32));
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

    use crate::window_snap::{MonitorBox, Placement};

    fn test_store() -> crate::SharedStore {
        Arc::new(client_core::store::Store::open(std::path::Path::new(":memory:")).unwrap())
    }

    #[test]
    fn copy_toast_metrics_scale_from_monitor_size() {
        let reference = MonitorBox {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
            name: None,
            scale: 1.0,
        };
        assert_eq!(super::copy_toast_metrics(&reference), (438, 80, 120));

        let half_size = MonitorBox {
            w: 960,
            h: 540,
            ..reference
        };
        assert_eq!(super::copy_toast_metrics(&half_size), (219, 40, 60));
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
