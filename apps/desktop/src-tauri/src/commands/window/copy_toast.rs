//! The detached copy-toast window: a compact Raycast-style confirmation that
//! pops near the bottom of the cursor's monitor and auto-hides.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Size, WebviewUrl, WebviewWindowBuilder,
};
use tauri_specta::Event;

use super::geometry::cursor_monitor;
use crate::events::CopyToastRequested;
use crate::window_snap::MonitorBox;

pub const COPY_TOAST_LABEL: &str = "copy-toast";
const COPY_TOAST_REFERENCE_WIDTH_PX: f64 = 1920.0;
const COPY_TOAST_REFERENCE_HEIGHT_PX: f64 = 1080.0;
const COPY_TOAST_MAX_WIDTH_PX: u32 = 438;
const COPY_TOAST_MAX_HEIGHT_PX: u32 = 80;
const COPY_TOAST_WIDTH_RATIO: f64 = 438.0 / COPY_TOAST_REFERENCE_WIDTH_PX;
const COPY_TOAST_HEIGHT_RATIO: f64 = 80.0 / COPY_TOAST_REFERENCE_HEIGHT_PX;
const COPY_TOAST_BOTTOM_OFFSET_RATIO: f64 = 120.0 / COPY_TOAST_REFERENCE_HEIGHT_PX;
const COPY_TOAST_DURATION_MS: u64 = 1800;

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

pub(crate) fn copy_toast_metrics(m: &MonitorBox) -> (u32, u32, i32) {
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

#[cfg(test)]
mod tests {
    use crate::window_snap::MonitorBox;

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
}
