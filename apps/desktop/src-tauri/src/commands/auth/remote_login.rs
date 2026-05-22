use tauri::{AppHandle, State};

/// Core logic for approving a pending device-code login.
/// Extracted so tests can drive it without a live Tauri AppHandle.
pub(crate) async fn approve_remote_login_impl(
    user_code: &str,
    relay_url: &str,
    token: &str,
    pending: &crate::auth::state::PendingCodesHandle,
) -> Result<(), String> {
    let client = client_core::http::RestClient::new(relay_url, token, crate::build_client_info())
        .map_err(|e| e.to_string())?;
    client
        .complete_device_code(user_code)
        .await
        .map_err(|e| e.to_string())?;
    crate::auth::state::remove_pending_code(pending, user_code);
    Ok(())
}

/// Core logic for denying a pending device-code login.
/// Extracted so tests can drive it without a live Tauri AppHandle.
pub(crate) async fn deny_remote_login_impl(
    user_code: &str,
    relay_url: &str,
    token: &str,
    pending: &crate::auth::state::PendingCodesHandle,
) -> Result<(), String> {
    let client = client_core::http::RestClient::new(relay_url, token, crate::build_client_info())
        .map_err(|e| e.to_string())?;
    client
        .deny_device_code(user_code)
        .await
        .map_err(|e| e.to_string())?;
    crate::auth::state::remove_pending_code(pending, user_code);
    Ok(())
}

/// approve_remote_login — accept a pending device-code request and clear it
/// from the local pending list.
///
/// Calls `POST /auth/device-code/complete` on the relay with bearer auth,
/// then removes the matching entry from PendingCodesHandle.
#[tauri::command]
#[specta::specta]
pub async fn approve_remote_login(
    user_code: String,
    _app: AppHandle,
    pending: State<'_, crate::auth::state::PendingCodesHandle>,
) -> Result<(), String> {
    let cfg = crate::protocol::Config::load().unwrap_or_default();
    if cfg.token.is_empty() {
        return Err("not signed in".into());
    }
    approve_remote_login_impl(&user_code, &cfg.relay_url, &cfg.token, pending.inner()).await
}

/// deny_remote_login — reject a pending device-code request and clear it
/// from the local pending list.
///
/// Calls `POST /cinch.v1.AuthService/DeviceCodeDeny` (Connect-RPC unary)
/// on the relay with bearer auth, then removes the matching entry from
/// PendingCodesHandle.
#[tauri::command]
#[specta::specta]
pub async fn deny_remote_login(
    user_code: String,
    _app: AppHandle,
    pending: State<'_, crate::auth::state::PendingCodesHandle>,
) -> Result<(), String> {
    let cfg = crate::protocol::Config::load().unwrap_or_default();
    if cfg.token.is_empty() {
        return Err("not signed in".into());
    }
    deny_remote_login_impl(&user_code, &cfg.relay_url, &cfg.token, pending.inner()).await
}
