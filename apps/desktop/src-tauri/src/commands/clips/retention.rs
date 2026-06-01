use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::SharedStore;
use client_core::store::queries;
use client_core::store::settings;

// ---------------------------------------------------------------------------
// Retention config — backed by client-core SharedStore settings table.
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

/// Testable inner: read both retention values, defaulting missing / unparseable
/// entries to [`DEFAULT_RETENTION_DAYS`] (D-05).
fn get_retention_config_inner(
    store: &client_core::store::Store,
) -> Result<RetentionConfig, String> {
    let local_days = settings::local_retention_days(store)
        .map_err(|e| e.to_string())?
        .unwrap_or(DEFAULT_RETENTION_DAYS);
    let remote_days = settings::remote_retention_days(store)
        .map_err(|e| e.to_string())?
        .unwrap_or(DEFAULT_RETENTION_DAYS);
    Ok(RetentionConfig {
        local_days,
        remote_days,
    })
}

/// Testable inner: validate inputs fall in `[MIN_RETENTION_DAYS, MAX_RETENTION_DAYS]`
/// (V5 input-validation gate, T-06-01) then persist both via the settings table.
/// Out-of-range input is rejected BEFORE any write, so an invalid call cannot mutate state.
fn set_retention_config_inner(
    store: &client_core::store::Store,
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
    settings::set_local_retention_days(store, local_days).map_err(|e| e.to_string())?;
    settings::set_remote_retention_days(store, remote_days).map_err(|e| e.to_string())?;
    Ok(())
}

// --- Retention tauri commands (plan 01-06) ---

#[tauri::command]
#[specta::specta]
pub fn get_retention_config(store: State<'_, SharedStore>) -> Result<RetentionConfig, String> {
    get_retention_config_inner(&store)
}

#[tauri::command]
#[specta::specta]
pub async fn set_retention_config(
    store: State<'_, SharedStore>,
    local_days: i64,
    remote_days: i64,
) -> Result<(), String> {
    set_retention_config_inner(&store, local_days, remote_days)?;
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
pub fn preview_retention_change(store: State<'_, SharedStore>, days: i64) -> Result<i64, String> {
    if !(MIN_RETENTION_DAYS..=MAX_RETENTION_DAYS).contains(&days) {
        return Err(format!(
            "preview days out of range [{}, {}]: {}",
            MIN_RETENTION_DAYS, MAX_RETENTION_DAYS, days,
        ));
    }
    let cutoff = chrono::Utc::now().timestamp() - days * 86_400;
    queries::count_clips_before(&store, cutoff).map_err(|e| e.to_string())
}

/// Wipe every clip row. Returns the number of rows deleted.
/// Used by the "Clear local history" Settings button (PRV-03).
#[tauri::command]
#[specta::specta]
pub fn clear_local_history(store: State<'_, SharedStore>) -> Result<i64, String> {
    // Only clears the unified store. Legacy-DB media cascade-cleanup (removing
    // on-disk media files that were tracked by the old desktop DB) is intentionally
    // deferred until the legacy store is fully retired (a later task), per the
    // clean-cutover decision (Approach C, 2026-06-01).
    queries::clear_all_clips(&store).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_core::store::Store;

    fn test_store() -> Store {
        Store::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn retention_roundtrip() {
        let store = test_store();
        set_retention_config_inner(&store, 14, 60).unwrap();
        let cfg = get_retention_config_inner(&store).unwrap();
        assert_eq!(cfg.local_days, 14);
        assert_eq!(cfg.remote_days, 60);
    }

    #[test]
    fn retention_defaults_to_30_when_missing() {
        let store = test_store();
        let cfg = get_retention_config_inner(&store).unwrap();
        assert_eq!(cfg.local_days, DEFAULT_RETENTION_DAYS);
        assert_eq!(cfg.remote_days, DEFAULT_RETENTION_DAYS);
    }

    #[test]
    fn retention_out_of_range_low() {
        let store = test_store();
        assert!(set_retention_config_inner(&store, 3, 30).is_err());
        // Invalid write must not persist — missing keys fall through to defaults.
        let cfg = get_retention_config_inner(&store).unwrap();
        assert_eq!(
            cfg.local_days, DEFAULT_RETENTION_DAYS,
            "invalid write must not persist"
        );
    }

    #[test]
    fn retention_out_of_range_high() {
        let store = test_store();
        assert!(set_retention_config_inner(&store, 30, 1000).is_err());
    }

    #[test]
    fn retention_accepts_boundary_values() {
        let store = test_store();
        assert!(set_retention_config_inner(&store, MIN_RETENTION_DAYS, MAX_RETENTION_DAYS).is_ok());
        let cfg = get_retention_config_inner(&store).unwrap();
        assert_eq!(cfg.local_days, MIN_RETENTION_DAYS);
        assert_eq!(cfg.remote_days, MAX_RETENTION_DAYS);
    }
}
