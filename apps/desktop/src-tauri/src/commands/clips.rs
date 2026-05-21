use std::sync::Arc;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::clipboard::ClipboardService;
use crate::protocol::{ConfigInfo, DeviceInfo, MultiConfigHandle};
use crate::store::db::{Database, SourceAlertSetting, SourceSetting};
use crate::sync_status::WsStatus;
use client_core::store::models::{SourceRow, StoredClip};
use client_core::store::queries;

/// Alias imported from lib.rs
use crate::SharedStore;

// ---------------------------------------------------------------------------
// Local wire type kept for Specta / frontend compatibility.
//
// `StoredClip` from client_core uses `content: Option<Vec<u8>>` (binary-safe).
// The frontend was built against `LocalClip` (String content + extra metadata).
// Rather than updating every .tsx file in this task, we keep this shape and
// convert with `stored_to_local` below.
//
// TODO(phase 5): migrate the frontend to consume StoredClip directly and
// delete LocalClip from here.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LocalClip {
    pub id: String,
    pub user_id: String,
    pub content: String,
    pub content_type: String,
    pub source: String,
    pub label: String,
    pub byte_size: i64,
    pub media_path: Option<String>,
    pub created_at: i64, // unix seconds (frontend convention)
    pub synced: bool,
    pub is_pinned: bool,
    pub pin_note: Option<String>,
    pub received_at: i64,
}

/// Normalize a stored `content_type` to the canonical 4-string wire vocab.
///
/// Pre-0510e1f desktop builds emitted MIME-style values (`"text/plain"`,
/// `"image/png"`) and the relay's open-string column preserved them. The
/// frontend dispatches on strict equality (`=== "image"`, `=== "text"`),
/// so we collapse MIME prefixes here — at the Rust → frontend boundary —
/// rather than spreading the defense across every React component.
pub(crate) fn normalize_content_type(ct: String) -> String {
    if ct.starts_with("image") {
        return "image".to_string();
    }
    if ct.starts_with("text") {
        return "text".to_string();
    }
    ct
}

/// Convert a `StoredClip` (client-core, ms timestamps) to a `LocalClip`
/// (desktop frontend, second timestamps).
fn stored_to_local(c: StoredClip) -> LocalClip {
    let content = c
        .content
        .as_deref()
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("")
        .to_string();
    // client-core stores created_at in milliseconds; frontend expects seconds.
    let created_at_secs = c.created_at / 1000;
    LocalClip {
        id: c.id,
        user_id: String::new(),
        content,
        content_type: normalize_content_type(c.content_type),
        source: c.source,
        label: String::new(),
        byte_size: c.byte_size,
        media_path: c.media_path,
        created_at: created_at_secs,
        synced: true,
        is_pinned: c.pinned,
        pin_note: None, // pinned_at is an i64 in StoredClip; notes not stored
        received_at: created_at_secs,
    }
}

impl LocalClip {
    /// Convert from the legacy `store::models::LocalClip` that `ws.rs` and
    /// `clipboard/monitor.rs` still produce (Task 4.3 will remove those callers).
    pub fn from_legacy(l: crate::store::models::LocalClip) -> Self {
        Self {
            id: l.id,
            user_id: l.user_id,
            content: l.content,
            content_type: normalize_content_type(l.content_type),
            source: l.source,
            label: l.label,
            byte_size: l.byte_size,
            media_path: l.media_path,
            created_at: l.created_at,
            synced: l.synced,
            is_pinned: l.is_pinned,
            pin_note: l.pin_note,
            received_at: l.received_at,
        }
    }
}

// ---------------------------------------------------------------------------
// SourceInfo — returned to the frontend; matches the old desktop shape.
// client_core::store::models::SourceRow has the same fields (source,
// clip_count, last_seen) so we forward it directly but keep the desktop name
// for Specta compatibility.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SourceInfo {
    pub source: String,
    pub clip_count: i64,
    pub last_seen: i64,
}

fn source_row_to_info(r: SourceRow) -> SourceInfo {
    SourceInfo {
        source: r.source,
        clip_count: r.clip_count,
        // client-core stores last_seen in milliseconds; convert to seconds.
        last_seen: r.last_seen.unwrap_or(0) / 1000,
    }
}

// ---------------------------------------------------------------------------
// Retention config — still backed by the legacy SQLite settings table.
// client-core has retention_prefs (per-device) but no equivalent of the
// desktop's local_retention_days / remote_retention_days scalar pair.
// TODO(phase 5): port to client-core retention_prefs table.
// ---------------------------------------------------------------------------

/// Settings-pane retention config (plan 01-06).
///
/// `local_days` = rolling window for the local SQLite cache.
/// `remote_days` = rolling window for relay-stored clips.
/// Default is 30 days per D-05; clamp range is `[7, 365]` per V5.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct RetentionConfig {
    pub local_days: i64,
    pub remote_days: i64,
}

const DEFAULT_RETENTION_DAYS: i64 = 30;
const MIN_RETENTION_DAYS: i64 = 7;
const MAX_RETENTION_DAYS: i64 = 365;

/// Best-effort sync of remote_retention_days to the relay.
/// Fails silently — the relay will fall back to DEFAULT 30 if unreachable.
async fn sync_retention_to_relay(remote_days: i64) {
    let cfg = match crate::protocol::Config::load() {
        Ok(c) => c,
        Err(_) => return,
    };
    let token = match crate::auth::read_credentials(&cfg) {
        Ok(t) => t,
        Err(_) => return, // not authenticated — skip silently
    };
    if token.is_empty() {
        return;
    }

    let url = format!(
        "{}/devices/self/retention",
        cfg.relay_url.trim_end_matches('/')
    );
    let body = serde_json::json!({ "remote_retention_days": remote_days });

    let client = reqwest::Client::new();
    let _ = client
        .put(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&body)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;
    // Fire-and-forget: don't block the settings save
}

// --- Retention inner helpers (testable without tauri State wrapper) ---

/// Testable inner: read both retention values, defaulting missing / unparseable
/// entries to [`DEFAULT_RETENTION_DAYS`] (D-05).
fn get_retention_config_inner(db: &Database) -> Result<RetentionConfig, String> {
    let local_days = db
        .get_setting("local_retention_days")?
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(DEFAULT_RETENTION_DAYS);
    let remote_days = db
        .get_setting("remote_retention_days")?
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(DEFAULT_RETENTION_DAYS);
    Ok(RetentionConfig {
        local_days,
        remote_days,
    })
}

/// Testable inner: validate inputs fall in `[MIN_RETENTION_DAYS, MAX_RETENTION_DAYS]`
/// (V5 input-validation gate, T-06-01) then persist both via
/// `Database::set_setting`. Out-of-range input is rejected BEFORE any write,
/// so an invalid call cannot mutate state.
fn set_retention_config_inner(
    db: &Database,
    local_days: i64,
    remote_days: i64,
) -> Result<(), String> {
    if !(MIN_RETENTION_DAYS..=MAX_RETENTION_DAYS).contains(&local_days)
        || !(MIN_RETENTION_DAYS..=MAX_RETENTION_DAYS).contains(&remote_days)
    {
        return Err(format!(
            "retention out of range [{}, {}]: local={}, remote={}",
            MIN_RETENTION_DAYS, MAX_RETENTION_DAYS, local_days, remote_days,
        ));
    }
    db.set_setting("local_retention_days", &local_days.to_string())?;
    db.set_setting("remote_retention_days", &remote_days.to_string())?;
    Ok(())
}

// --- Retention tauri commands (plan 01-06) ---
// TODO(phase 5): port to client-core retention_prefs table (queries::set_retention /
// queries::list_retention). Legacy Database stays for now.

#[tauri::command]
#[specta::specta]
pub fn get_retention_config(db: State<'_, Arc<Database>>) -> Result<RetentionConfig, String> {
    get_retention_config_inner(&db)
}

#[tauri::command]
#[specta::specta]
pub async fn set_retention_config(
    db: State<'_, Arc<Database>>,
    local_days: i64,
    remote_days: i64,
) -> Result<(), String> {
    set_retention_config_inner(&db, local_days, remote_days)?;
    // PRV-02: best-effort relay sync — don't fail the local save if relay is unreachable
    let rd = remote_days;
    tauri::async_runtime::spawn(async move {
        sync_retention_to_relay(rd).await;
    });
    Ok(())
}

/// Return the number of clips that would be deleted if `local_retention_days`
/// were set to `days` right now. Backs the Settings-pane retroactive-purge
/// confirmation dialog.
///
/// `days` is clamped to `[MIN_RETENTION_DAYS, MAX_RETENTION_DAYS]` (T-06-02).
#[tauri::command]
#[specta::specta]
pub fn preview_retention_change(db: State<'_, Arc<Database>>, days: i64) -> Result<i64, String> {
    if !(MIN_RETENTION_DAYS..=MAX_RETENTION_DAYS).contains(&days) {
        return Err(format!(
            "preview days out of range [{}, {}]: {}",
            MIN_RETENTION_DAYS, MAX_RETENTION_DAYS, days,
        ));
    }
    let cutoff = chrono::Utc::now().timestamp() - days * 86_400;
    db.count_clips_before(cutoff)
}

/// Wipe every clip row + cascade-delete media files. Returns the number of
/// rows deleted. Used by the "Clear local history" Settings button (PRV-03).
#[tauri::command]
#[specta::specta]
pub fn clear_local_history(
    db: State<'_, Arc<Database>>,
    store: State<'_, SharedStore>,
) -> Result<i64, String> {
    // Clear both stores until the legacy DB is retired (Task 4.3+).
    let _ = queries::clear_all_clips(&store).map_err(|e| log::warn!("clear new store: {e}"));
    db.clear_all_clips()
}

// ---------------------------------------------------------------------------
// Clip read commands — delegated to client_core::store::queries
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn list_pinned_clips(store: State<'_, SharedStore>) -> Result<Vec<LocalClip>, String> {
    queries::list_clips(&store, None, None, None, true, 200)
        .map(|v| v.into_iter().map(stored_to_local).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn pin_clip(
    store: State<'_, SharedStore>,
    mc: State<'_, MultiConfigHandle>,
    id: String,
    note: Option<String>,
) -> Result<(), String> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    queries::set_pinned(&store, &id, true, now_ms).map_err(|e| e.to_string())?;
    if let Ok((relay_url, token)) = resolve_active_creds(&mc) {
        match client_core::http::RestClient::new(relay_url, token, crate::build_client_info()) {
            Ok(client) => {
                if let Err(e) = client.set_clip_pin(&id, true, note.as_deref()).await {
                    log::warn!("relay set_clip_pin failed for {}: {}", id, e);
                }
            }
            Err(e) => {
                log::warn!("could not build REST client for pin_clip {}: {}", id, e);
            }
        }
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn unpin_clip(
    store: State<'_, SharedStore>,
    mc: State<'_, MultiConfigHandle>,
    id: String,
) -> Result<(), String> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    queries::set_pinned(&store, &id, false, now_ms).map_err(|e| e.to_string())?;
    if let Ok((relay_url, token)) = resolve_active_creds(&mc) {
        match client_core::http::RestClient::new(relay_url, token, crate::build_client_info()) {
            Ok(client) => {
                if let Err(e) = client.set_clip_pin(&id, false, None).await {
                    log::warn!("relay unpin_clip failed for {}: {}", id, e);
                }
            }
            Err(e) => {
                log::warn!("could not build REST client for unpin_clip {}: {}", id, e);
            }
        }
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn list_clips(
    store: State<'_, SharedStore>,
    source: Option<String>,
    content_type: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<LocalClip>, String> {
    let clips = queries::list_clips(&store, source.as_deref(), limit, None, false, 50)
        .map_err(|e| e.to_string())?;

    // Optional client-side content_type filter (client-core query has no content_type filter yet).
    let filtered: Vec<LocalClip> = clips
        .into_iter()
        .map(stored_to_local)
        .filter(|c| {
            content_type
                .as_deref()
                .map(|ct| c.content_type == ct)
                .unwrap_or(true)
        })
        .collect();
    Ok(filtered)
}

#[tauri::command]
#[specta::specta]
pub fn search_clips(
    store: State<'_, SharedStore>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<LocalClip>, String> {
    queries::search_clips(&store, &query, limit.unwrap_or(50))
        .map(|v| v.into_iter().map(stored_to_local).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_sources(store: State<'_, SharedStore>) -> Result<Vec<SourceInfo>, String> {
    queries::list_sources(&store)
        .map(|v| v.into_iter().map(source_row_to_info).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_clip(
    store: State<'_, SharedStore>,
    mc: State<'_, MultiConfigHandle>,
    id: String,
) -> Result<(), String> {
    // Best-effort relay deletion: propagate to other devices via clip_deleted broadcast.
    // If the relay is unreachable, log and continue — relay TTL will eventually expire the clip.
    if let Ok((relay_url, token)) = resolve_active_creds(&mc) {
        match client_core::http::RestClient::new(relay_url, token, crate::build_client_info()) {
            Ok(client) => {
                if let Err(e) = client.delete_clip(&id).await {
                    log::warn!("relay delete_clip failed for {}: {}", id, e);
                }
            }
            Err(e) => {
                log::warn!("could not build REST client for delete_clip {}: {}", id, e);
            }
        }
    }
    queries::delete_clip(&store, &id).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_clip_count(store: State<'_, SharedStore>) -> Result<i64, String> {
    queries::clip_count(&store).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Config / auth info — no store dependency
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn get_config_info(mc: State<'_, MultiConfigHandle>) -> ConfigInfo {
    let guard = mc.lock().unwrap();
    let cfg = guard.to_active_config();
    ConfigInfo {
        relay_url: cfg.relay_url.clone(),
        user_id: cfg.user_id.clone(),
        hostname: cfg.hostname.clone(),
    }
}

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
// mark_clip_copied — TODO(phase 5): client-core has no copied_at column yet.
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn mark_clip_copied(db: State<'_, Arc<Database>>, id: String) -> Result<(), String> {
    db.mark_clip_copied(&id, chrono::Utc::now().timestamp())
}

// ---------------------------------------------------------------------------
// Clipboard write commands — no store dependency
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn copy_clip_to_clipboard(
    clipboard: State<'_, Arc<ClipboardService>>,
    content: String,
) -> Result<(), String> {
    clipboard.write_text(&content).map_err(|e| e.to_string())
}

/// Read an image clip's raw bytes from the store. Err if absent / not an image.
pub(crate) fn image_bytes_for(
    store: &client_core::store::Store,
    clip_id: &str,
) -> Result<Vec<u8>, String> {
    match queries::get_clip(store, clip_id) {
        Ok(Some(c)) if normalize_content_type(c.content_type.clone()) == "image" => c
            .content
            .filter(|b| !b.is_empty())
            .ok_or_else(|| "clip has no image bytes".to_string()),
        Ok(_) => Err(format!("no image clip with id {clip_id}")),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
#[specta::specta]
pub fn copy_image_to_clipboard(
    clipboard: State<'_, Arc<ClipboardService>>,
    store: State<'_, crate::SharedStore>,
    clip_id: String,
) -> Result<(), String> {
    let bytes = image_bytes_for(store.inner(), &clip_id)?;
    clipboard
        .write_image_png_bytes(&bytes)
        .map_err(|e| e.to_string())
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

// ---------------------------------------------------------------------------
// save_config — no store dependency
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn save_config(app: tauri::AppHandle, relay_url: String, token: String) -> Result<(), String> {
    if relay_url.trim().is_empty() || token.trim().is_empty() {
        return Err("relay_url and token are required".to_string());
    }
    let relay_url = relay_url.trim().trim_end_matches('/').to_string();
    let hostname = client_core::machine::hostname_or_unknown();

    let existing = crate::protocol::Config::load().unwrap_or_default();
    let user_id = if existing.user_id.is_empty() {
        "unknown-user".to_string()
    } else {
        existing.user_id
    };
    let device_id = if existing.active_device_id.is_empty() {
        "unknown-device".to_string()
    } else {
        existing.active_device_id
    };

    crate::auth::credential::write_credentials(
        &user_id,
        &device_id,
        token.trim(),
        &relay_url,
        &hostname,
    )
    .map_err(|e| format!("write_credentials: {}", e))?;

    log::info!("save_config succeeded");

    app.restart();
}

// ---------------------------------------------------------------------------
// Helper: extract (relay_url, token) from active MultiConfig profile
// ---------------------------------------------------------------------------

fn resolve_active_creds(mc: &State<'_, MultiConfigHandle>) -> Result<(String, String), String> {
    let guard = mc.lock().unwrap();
    let profile = guard.active_profile().ok_or("no active relay configured")?;
    let token = if profile.token.is_empty() {
        let cfg = profile.to_config();
        crate::auth::read_credentials(&cfg).map_err(|_| "not authenticated".to_string())?
    } else {
        profile.token.clone()
    };
    if token.is_empty() {
        return Err("not authenticated".to_string());
    }
    Ok((profile.relay_url.clone(), token))
}

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

// ---------------------------------------------------------------------------
// Global shortcut persistence (plan 03-04, D-08)
// TODO(phase 5): move to client-core meta/settings table.
// ---------------------------------------------------------------------------

const DEFAULT_GLOBAL_SHORTCUT: &str = "CmdOrCtrl+Shift+V";

/// Modifier key names recognized by Tauri's global-shortcut plugin.
const MODIFIER_NAMES: &[&str] = &[
    "cmd",
    "ctrl",
    "alt",
    "shift",
    "super",
    "meta",
    "commandorcontrol",
    "cmdorctrl",
];

/// Testable inner: read persisted global shortcut or return the default.
fn get_global_shortcut_inner(db: &Database) -> Result<String, String> {
    Ok(db
        .get_setting("global_shortcut")?
        .unwrap_or_else(|| DEFAULT_GLOBAL_SHORTCUT.to_string()))
}

/// Testable inner: validate and persist a global shortcut string (T-03-06).
///
/// Validation rules:
/// 1. Must contain at least one modifier key (Cmd, Ctrl, Alt, Shift, etc.)
/// 2. Must contain at least one regular (non-modifier) key
fn set_global_shortcut_inner(db: &Database, shortcut: &str) -> Result<(), String> {
    let parts: Vec<&str> = shortcut.split('+').collect();
    let has_modifier = parts
        .iter()
        .any(|p| MODIFIER_NAMES.contains(&p.to_lowercase().as_str()));
    if !has_modifier {
        return Err(
            "Shortcut must include at least one modifier key (Cmd, Ctrl, Alt, Shift)".to_string(),
        );
    }
    let has_regular_key = parts
        .iter()
        .any(|p| !MODIFIER_NAMES.contains(&p.to_lowercase().as_str()));
    if !has_regular_key {
        return Err("Shortcut must include a regular key (e.g., V, C, Space)".to_string());
    }
    db.set_setting("global_shortcut", shortcut)
}

#[tauri::command]
#[specta::specta]
pub fn get_global_shortcut(db: State<'_, Arc<Database>>) -> Result<String, String> {
    get_global_shortcut_inner(&db)
}

#[tauri::command]
#[specta::specta]
pub fn set_global_shortcut(db: State<'_, Arc<Database>>, shortcut: String) -> Result<(), String> {
    set_global_shortcut_inner(&db, &shortcut)
}

// ---------------------------------------------------------------------------
// WS status — no store dependency
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn get_ws_status(ws_status: State<'_, Arc<WsStatus>>) -> String {
    ws_status.get()
}

// ---------------------------------------------------------------------------
// Focus previous app — no store dependency
// ---------------------------------------------------------------------------

/// Restore focus to the app that was frontmost before Cinch was shown, then hide the
/// Cinch window. On non-macOS platforms this simply hides the window.
#[tauri::command]
#[specta::specta]
pub fn focus_previous_app(
    app: tauri::AppHandle,
    previous_pid: State<'_, crate::PreviousAppPid>,
) -> Result<(), String> {
    use tauri::Manager;
    #[cfg(target_os = "macos")]
    {
        let pid_opt = *previous_pid.lock().map_err(|e| e.to_string())?;
        if let Some(pid) = pid_opt {
            crate::activate_app_by_pid(pid);
        }
    }

    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests — unit tests for the testable inner functions (settings, shortcut).
// The clip-query commands are covered by the client-core wire-vector suite.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::db::Database;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_db() -> Database {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp: PathBuf =
            std::env::temp_dir().join(format!("cinch-cmd-test-{}-{}.db", std::process::id(), n));
        let _ = std::fs::remove_file(&tmp);
        Database::open(&tmp).unwrap()
    }

    #[test]
    fn retention_roundtrip() {
        let db = test_db();
        set_retention_config_inner(&db, 14, 60).unwrap();
        let cfg = get_retention_config_inner(&db).unwrap();
        assert_eq!(cfg.local_days, 14);
        assert_eq!(cfg.remote_days, 60);
    }

    #[test]
    fn retention_defaults_to_30_when_missing() {
        let db = test_db();
        let cfg = get_retention_config_inner(&db).unwrap();
        assert_eq!(cfg.local_days, DEFAULT_RETENTION_DAYS);
        assert_eq!(cfg.remote_days, DEFAULT_RETENTION_DAYS);
    }

    #[test]
    fn retention_out_of_range_low() {
        let db = test_db();
        assert!(set_retention_config_inner(&db, 3, 30).is_err());
        // Invalid write must not persist — missing keys fall through to defaults.
        let cfg = get_retention_config_inner(&db).unwrap();
        assert_eq!(
            cfg.local_days, DEFAULT_RETENTION_DAYS,
            "invalid write must not persist"
        );
    }

    #[test]
    fn retention_out_of_range_high() {
        let db = test_db();
        assert!(set_retention_config_inner(&db, 30, 1000).is_err());
    }

    #[test]
    fn retention_accepts_boundary_values() {
        let db = test_db();
        assert!(set_retention_config_inner(&db, MIN_RETENTION_DAYS, MAX_RETENTION_DAYS).is_ok());
        let cfg = get_retention_config_inner(&db).unwrap();
        assert_eq!(cfg.local_days, MIN_RETENTION_DAYS);
        assert_eq!(cfg.remote_days, MAX_RETENTION_DAYS);
    }

    // --- Global shortcut tests (plan 03-04) ---

    #[test]
    fn global_shortcut_defaults_when_missing() {
        let db = test_db();
        let s = get_global_shortcut_inner(&db).unwrap();
        assert_eq!(s, DEFAULT_GLOBAL_SHORTCUT);
    }

    #[test]
    fn global_shortcut_roundtrip() {
        let db = test_db();
        set_global_shortcut_inner(&db, "CmdOrCtrl+Shift+B").unwrap();
        let s = get_global_shortcut_inner(&db).unwrap();
        assert_eq!(s, "CmdOrCtrl+Shift+B");
    }

    #[test]
    fn global_shortcut_rejects_no_modifier() {
        let db = test_db();
        let err = set_global_shortcut_inner(&db, "V").unwrap_err();
        assert!(
            err.contains("modifier"),
            "error should mention modifier: {}",
            err
        );
    }

    #[test]
    fn global_shortcut_rejects_modifier_only() {
        let db = test_db();
        let err = set_global_shortcut_inner(&db, "Cmd+Shift").unwrap_err();
        assert!(
            err.contains("regular key"),
            "error should mention regular key: {}",
            err
        );
    }

    #[test]
    fn global_shortcut_accepts_alt_combo() {
        let db = test_db();
        assert!(set_global_shortcut_inner(&db, "Alt+Space").is_ok());
        assert_eq!(get_global_shortcut_inner(&db).unwrap(), "Alt+Space");
    }

    // --- stored_to_local bridge tests ---

    #[test]
    fn stored_to_local_converts_ms_to_seconds() {
        let sc = StoredClip {
            id: "01JTEST00000000000000000000".to_string(),
            source: "local".to_string(),
            source_key: None,
            content_type: "text".to_string(),
            content: Some(b"hello".to_vec()),
            media_path: None,
            byte_size: 5,
            created_at: 1_777_614_529_000, // ms
            pinned: false,
            pinned_at: None,
            synced: true,
        };
        let lc = stored_to_local(sc);
        assert_eq!(lc.created_at, 1_777_614_529); // seconds
        assert_eq!(lc.content, "hello");
        assert!(!lc.is_pinned);
    }

    #[test]
    fn stored_to_local_binary_content_is_empty_string() {
        let sc = StoredClip {
            id: "01JTEST00000000000000000001".to_string(),
            source: "local".to_string(),
            source_key: None,
            content_type: "image".to_string(),
            content: None,
            media_path: Some("media/shot.png".to_string()),
            byte_size: 1024,
            created_at: 1_000_000_000,
            pinned: false,
            pinned_at: None,
            synced: true,
        };
        let lc = stored_to_local(sc);
        assert_eq!(lc.content, "");
        assert_eq!(lc.media_path.as_deref(), Some("media/shot.png"));
    }

    #[test]
    fn normalize_content_type_collapses_mime_prefixes() {
        // Canonical 4-string vocab passes through unchanged.
        assert_eq!(normalize_content_type("text".into()), "text");
        assert_eq!(normalize_content_type("code".into()), "code");
        assert_eq!(normalize_content_type("url".into()), "url");
        assert_eq!(normalize_content_type("image".into()), "image");
        // Legacy MIME values from pre-0510e1f desktop builds collapse.
        assert_eq!(normalize_content_type("text/plain".into()), "text");
        assert_eq!(normalize_content_type("text/html".into()), "text");
        assert_eq!(normalize_content_type("image/png".into()), "image");
        assert_eq!(normalize_content_type("image/jpeg".into()), "image");
        // Unknown values flow through verbatim — defense, not censorship.
        assert_eq!(normalize_content_type("audio".into()), "audio");
        assert_eq!(normalize_content_type("".into()), "");
    }

    #[test]
    fn stored_to_local_normalizes_legacy_mime() {
        let sc = StoredClip {
            id: "01JTEST00000000000000000002".to_string(),
            source: "remote:laptop".to_string(),
            source_key: None,
            content_type: "text/plain".to_string(),
            content: Some(b"legacy".to_vec()),
            media_path: None,
            byte_size: 6,
            created_at: 1_700_000_000_000,
            pinned: false,
            pinned_at: None,
            synced: true,
        };
        let lc = stored_to_local(sc);
        assert_eq!(
            lc.content_type, "text",
            "MIME-style stored values must surface as canonical to the frontend"
        );
    }

    #[test]
    fn image_bytes_for_returns_image_row_bytes() {
        let s = client_core::store::Store::open(std::path::Path::new(":memory:")).unwrap();
        let png = vec![0x89u8, 0x50, 0x4E, 0x47];
        client_core::store::queries::insert_clip(
            &s,
            &StoredClip {
                id: "i1".into(),
                source: "remote:t".into(),
                source_key: None,
                content_type: "image".into(),
                content: Some(png.clone()),
                media_path: None,
                byte_size: 0,
                created_at: 1,
                pinned: false,
                pinned_at: None,
                synced: true,
            },
        )
        .unwrap();
        assert_eq!(super::image_bytes_for(&s, "i1").unwrap(), png);
        assert!(super::image_bytes_for(&s, "missing").is_err());
    }

    #[test]
    fn image_bytes_for_accepts_legacy_mime_content_type() {
        let s = client_core::store::Store::open(std::path::Path::new(":memory:")).unwrap();
        let png = vec![0x89u8, 0x50, 0x4E, 0x47];
        client_core::store::queries::insert_clip(
            &s,
            &StoredClip {
                id: "legacy1".into(),
                source: "remote:peer".into(),
                source_key: None,
                content_type: "image/png".into(),
                content: Some(png.clone()),
                media_path: None,
                byte_size: 0,
                created_at: 1,
                pinned: false,
                pinned_at: None,
                synced: true,
            },
        )
        .unwrap();
        assert_eq!(super::image_bytes_for(&s, "legacy1").unwrap(), png);
    }
}
