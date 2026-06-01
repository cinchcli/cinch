use tauri::State;

use crate::clipboard::ClipboardService;
use crate::SharedStore;
use client_core::store::settings;
use client_core::store::settings::{SourceAlertSetting, SourceSetting};

// ---------------------------------------------------------------------------
// Source-level settings — backed by client-core SharedStore (settings table).
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn get_source_auto_copy(store: State<'_, SharedStore>, source: String) -> Result<bool, String> {
    settings::is_source_auto_copy(&store, &source).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn set_source_auto_copy(
    store: State<'_, SharedStore>,
    source: String,
    enabled: bool,
) -> Result<(), String> {
    settings::set_source_auto_copy(&store, &source, enabled).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_all_source_settings(
    store: State<'_, SharedStore>,
) -> Result<Vec<SourceSetting>, String> {
    settings::all_source_settings(&store).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_source_alert_enabled(
    store: State<'_, SharedStore>,
    source: String,
) -> Result<bool, String> {
    settings::is_source_alert_enabled(&store, &source).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn set_source_alert_enabled(
    store: State<'_, SharedStore>,
    source: String,
    enabled: bool,
) -> Result<(), String> {
    settings::set_source_alert_enabled(&store, &source, enabled).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_all_source_alert_settings(
    store: State<'_, SharedStore>,
) -> Result<Vec<SourceAlertSetting>, String> {
    settings::all_source_alert_settings(&store).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Excluded-apps setting — backed by client-core SharedStore (settings table).
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn get_excluded_apps(
    store: State<'_, SharedStore>,
    clipboard: State<'_, std::sync::Arc<ClipboardService>>,
) -> Result<Vec<String>, String> {
    let apps = settings::excluded_apps(&store).map_err(|e| e.to_string())?;
    if apps.is_empty() {
        Ok(clipboard.default_excluded_apps())
    } else {
        Ok(apps)
    }
}

#[tauri::command]
#[specta::specta]
pub fn set_excluded_apps(store: State<'_, SharedStore>, apps: Vec<String>) -> Result<(), String> {
    settings::set_excluded_apps(&store, &apps).map_err(|e| e.to_string())
}
