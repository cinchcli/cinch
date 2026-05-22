use tauri::State;

use super::resolve_active_creds;
use crate::protocol::{DeviceInfo, MultiConfigHandle};

// ---------------------------------------------------------------------------
// Device management commands — delegated to RestClient
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub async fn list_devices(mc: State<'_, MultiConfigHandle>) -> Result<Vec<DeviceInfo>, String> {
    let (relay_url, token) = resolve_active_creds(&mc)?;
    let client = client_core::http::RestClient::new(relay_url, token, crate::build_client_info())
        .map_err(|e| format!("build client: {}", e))?;
    client
        .list_devices()
        .await
        .map_err(|e| format!("list_devices: {}", e))
}

#[tauri::command]
#[specta::specta]
pub async fn set_device_nickname(
    mc: State<'_, MultiConfigHandle>,
    device_id: String,
    nickname: String,
) -> Result<(), String> {
    let (relay_url, token) = resolve_active_creds(&mc)?;
    let client = client_core::http::RestClient::new(relay_url, token, crate::build_client_info())
        .map_err(|e| format!("build client: {}", e))?;
    client
        .set_device_nickname(&device_id, &nickname)
        .await
        .map_err(|e| format!("set_device_nickname: {}", e))
}

#[tauri::command]
#[specta::specta]
pub async fn revoke_device(
    mc: State<'_, MultiConfigHandle>,
    device_id: String,
) -> Result<(), String> {
    let (relay_url, token) = resolve_active_creds(&mc)?;
    let client = client_core::http::RestClient::new(relay_url, token, crate::build_client_info())
        .map_err(|e| format!("build client: {}", e))?;
    client
        .revoke_device(&device_id)
        .await
        .map_err(|e| format!("revoke_device: {}", e))
}
