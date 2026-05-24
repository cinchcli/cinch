use tauri::{AppHandle, State};
use tauri_specta::Event;

use super::resolve_active_creds;
use super::DeviceCacheHandle;
use crate::events::DevicesChanged;
use crate::protocol::{DeviceInfo, MultiConfigHandle};

// ---------------------------------------------------------------------------
// Device management commands — delegated to RestClient
// ---------------------------------------------------------------------------
//
// `list_devices` is read through a 30-second in-memory TTL cache. The desktop
// polls every 5 seconds; without the cache that's an unconditional relay
// round trip on every tick. Mutations below explicitly invalidate the cache
// so a follow-up read after a rename or revoke surfaces the new state.

#[tauri::command]
#[specta::specta]
pub async fn list_devices(
    mc: State<'_, MultiConfigHandle>,
    cache: State<'_, DeviceCacheHandle>,
) -> Result<Vec<DeviceInfo>, String> {
    if let Some(cached) = cache.get() {
        return Ok(cached);
    }

    let (relay_url, token) = resolve_active_creds(&mc)?;
    let client = client_core::http::RestClient::new(relay_url, token, crate::build_client_info())
        .map_err(|e| format!("build client: {}", e))?;
    let devices = client
        .list_devices()
        .await
        .map_err(|e| format!("list_devices: {}", e))?;

    cache.insert(devices.clone());
    Ok(devices)
}

#[tauri::command]
#[specta::specta]
pub async fn set_device_nickname(
    app: AppHandle,
    mc: State<'_, MultiConfigHandle>,
    cache: State<'_, DeviceCacheHandle>,
    device_id: String,
    nickname: String,
) -> Result<(), String> {
    let (relay_url, token) = resolve_active_creds(&mc)?;
    let client = client_core::http::RestClient::new(relay_url, token, crate::build_client_info())
        .map_err(|e| format!("build client: {}", e))?;
    client
        .set_device_nickname(&device_id, &nickname)
        .await
        .map_err(|e| format!("set_device_nickname: {}", e))?;
    cache.invalidate();
    if let Err(e) = DevicesChanged.emit(&app) {
        log::warn!("DevicesChanged emit failed: {}", e);
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn revoke_device(
    app: AppHandle,
    mc: State<'_, MultiConfigHandle>,
    cache: State<'_, DeviceCacheHandle>,
    device_id: String,
) -> Result<(), String> {
    let (relay_url, token) = resolve_active_creds(&mc)?;
    let client = client_core::http::RestClient::new(relay_url, token, crate::build_client_info())
        .map_err(|e| format!("build client: {}", e))?;
    client
        .revoke_device(&device_id)
        .await
        .map_err(|e| format!("revoke_device: {}", e))?;
    cache.invalidate();
    if let Err(e) = DevicesChanged.emit(&app) {
        log::warn!("DevicesChanged emit failed: {}", e);
    }
    Ok(())
}
