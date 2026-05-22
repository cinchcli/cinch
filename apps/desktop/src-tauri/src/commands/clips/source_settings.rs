use std::sync::Arc;

use tauri::State;

use crate::clipboard::ClipboardService;
use crate::store::db::{Database, SourceAlertSetting, SourceSetting};

// ---------------------------------------------------------------------------
// Source-level settings — still backed by legacy Database.
// client-core has alert_prefs but not auto_copy; keeping both on the legacy
// store avoids half-migration. TODO(phase 5): move to client-core queries.
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn get_source_auto_copy(db: State<'_, Arc<Database>>, source: String) -> Result<bool, String> {
    db.is_source_auto_copy(&source)
}

#[tauri::command]
#[specta::specta]
pub fn set_source_auto_copy(
    db: State<'_, Arc<Database>>,
    source: String,
    enabled: bool,
) -> Result<(), String> {
    db.set_source_auto_copy(&source, enabled)
}

#[tauri::command]
#[specta::specta]
pub fn get_all_source_settings(db: State<'_, Arc<Database>>) -> Result<Vec<SourceSetting>, String> {
    db.get_all_source_settings()
}

#[tauri::command]
#[specta::specta]
pub fn get_source_alert_enabled(
    db: State<'_, Arc<Database>>,
    source: String,
) -> Result<bool, String> {
    db.is_source_alert_enabled(&source)
}

#[tauri::command]
#[specta::specta]
pub fn set_source_alert_enabled(
    db: State<'_, Arc<Database>>,
    source: String,
    enabled: bool,
) -> Result<(), String> {
    db.set_source_alert_enabled(&source, enabled)
}

#[tauri::command]
#[specta::specta]
pub fn get_all_source_alert_settings(
    db: State<'_, Arc<Database>>,
) -> Result<Vec<SourceAlertSetting>, String> {
    db.get_all_source_alert_settings()
}

// ---------------------------------------------------------------------------
// Excluded-apps setting — backed by legacy Database.
// TODO(phase 5): move to client-core meta/settings table.
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn get_excluded_apps(
    db: State<'_, Arc<Database>>,
    clipboard: State<'_, Arc<ClipboardService>>,
) -> Result<Vec<String>, String> {
    match db.get_setting("excluded_apps")? {
        Some(json) => {
            serde_json::from_str(&json).map_err(|e| format!("parse excluded_apps: {}", e))
        }
        None => Ok(clipboard.default_excluded_apps()),
    }
}

#[tauri::command]
#[specta::specta]
pub fn set_excluded_apps(db: State<'_, Arc<Database>>, apps: Vec<String>) -> Result<(), String> {
    let json =
        serde_json::to_string(&apps).map_err(|e| format!("serialize excluded_apps: {}", e))?;
    db.set_setting("excluded_apps", &json)
}
