//! Tauri commands backing the version-badge UI.
//!
//! `get_latest_versions` is the cache reader the React layer calls on
//! mount. If the cache is stale it spawns a background refresh and
//! emits `LatestVersionsUpdated` when fresh data arrives, so the badge
//! row reactively updates without the user reloading.
//!
//! `get_device_version_status` is a thin wrapper around the pure
//! `update_check::compare` helper, exposed as a command so the React
//! side does not duplicate the semver-trim logic.
//!
//! `run_self_update` runs the tauri-plugin-updater check + download
//! flow. Only the user's own desktop row should call this; the frontend
//! gates it on `isOwnDesktop && status === Outdated`.

use tauri_plugin_updater::UpdaterExt;
use tauri_specta::Event;

use crate::events::LatestVersionsUpdated;
use crate::update_check::{
    compare, fetch_and_cache, is_stale, load_cache, LatestVersions, VersionStatus,
};

#[tauri::command]
#[specta::specta]
pub async fn get_latest_versions(app: tauri::AppHandle) -> LatestVersions {
    let cache = load_cache(&app);
    if is_stale(&cache) {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move {
            let updated = fetch_and_cache(app2.clone()).await;
            let _ = LatestVersionsUpdated(updated).emit(&app2);
        });
    }
    cache
}

#[tauri::command]
#[specta::specta]
pub fn get_device_version_status(
    reported: Option<String>,
    client_type: Option<String>,
    latest: LatestVersions,
) -> VersionStatus {
    compare(reported.as_deref(), client_type.as_deref(), &latest)
}

#[tauri::command]
#[specta::specta]
pub async fn run_self_update(app: tauri::AppHandle) -> Result<(), String> {
    run_self_update_inner(app).await
}

/// Same logic as `run_self_update` but callable from non-command contexts
/// (e.g. the system tray menu handler).
pub async fn run_self_update_inner(app: tauri::AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    let Some(update) = update else { return Ok(()) };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}
