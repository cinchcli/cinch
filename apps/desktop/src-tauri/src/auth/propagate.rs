//! Cross-process credential propagation via FS watcher on ~/.cinch/config.json.
//! On `credential_version` bump, constructs the canonical AuthState from the fresh Config
//! snapshot and funnels through `crate::auth::transition()` — NEVER `app.emit(...)` directly.
//! (BLOCKER 1 remediation: single emitter per D-13.)

use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use log::{info, warn};
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use tauri::AppHandle;
use tauri_specta::Event;

use crate::auth::credential::CredentialError;
use crate::auth::{self, credential, AuthState, AuthStateHandle};

/// Spawn the FS watcher. Must be called exactly once during app boot (from lib.rs::run).
///
/// `auth_state_handle` is the shared `Arc<Mutex<AuthState>>` managed by Tauri; the watcher
/// funnels all state changes through `crate::auth::transition(&app, &handle, next)` so the
/// discriminated-union `{ variant, payload }` shape is honored for React subscribers.
///
/// Tracks the last-seen `credential_version` in a local mutex; on every debounced event
/// for `config.json`, re-reads the file, compares version, and if bumped calls `transition()`
/// with the next canonical AuthState (Authenticated or LocalOnly per D-15).
pub fn spawn_credential_watcher(
    app: AppHandle,
    auth_state_handle: AuthStateHandle,
) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("cannot determine home directory")?;
    let cinch_dir = home.join(".cinch");
    std::fs::create_dir_all(&cinch_dir).map_err(|e| format!("create ~/.cinch: {}", e))?;

    // Seed the watcher with the current on-disk version so the first event after startup
    // only fires if the file has actually changed since boot.
    let initial_version: u64 = match crate::protocol::Config::load() {
        Ok(cfg) => cfg.credential_version,
        Err(_) => 0,
    };
    let last_version = Arc::new(StdMutex::new(initial_version));

    let (tx, rx) = std::sync::mpsc::channel::<DebounceEventResult>();
    let mut debouncer = new_debouncer(Duration::from_millis(400), None, move |result| {
        let _ = tx.send(result);
    })
    .map_err(|e| format!("debouncer init: {}", e))?;

    debouncer
        .watch(&cinch_dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("watcher start: {}", e))?;

    let app_clone = app.clone();
    let last_version_clone = last_version.clone();
    let handle_clone = auth_state_handle.clone();

    std::thread::spawn(move || {
        // Keep the debouncer alive for the lifetime of the thread — dropping it
        // unregisters the watcher silently.
        let _keep_alive = debouncer;

        while let Ok(result) = rx.recv() {
            let events = match result {
                Ok(events) => events,
                Err(errs) => {
                    for e in errs {
                        warn!("fs-watch error: {}", e);
                    }
                    continue;
                }
            };
            let touched_config = events.iter().any(|ev| {
                ev.paths
                    .iter()
                    .any(|p| p.file_name().map(|n| n == "config.json").unwrap_or(false))
            });
            if !touched_config {
                continue;
            }

            let cfg = match crate::protocol::Config::load() {
                Ok(c) => c,
                Err(e) => {
                    warn!("config reload failed after fs event: {}", e);
                    continue;
                }
            };
            let mut guard = last_version_clone.lock().unwrap();
            if cfg.credential_version <= *guard {
                // No version bump — debounced event was a no-op write or touch (T-2-12 mitigation).
                continue;
            }
            *guard = cfg.credential_version;
            drop(guard);

            info!(
                "credential_version bumped to {} — funneling through transition()",
                cfg.credential_version
            );

            // Capture the prior state BEFORE transition so we can detect
            // LocalOnly → Authenticated, which is the signal that creds came
            // from the CLI (or another out-of-process writer).
            let was_local_only = matches!(
                handle_clone.lock().map(|g| g.clone()).ok(),
                Some(AuthState::LocalOnly)
            );

            // Construct the canonical next AuthState from the fresh Config snapshot.
            // D-15 row "FS watcher credential_version up" → re-read creds; resulting state
            // is Authenticated (if creds resolvable) or LocalOnly (if torn down).
            let next = next_state_from_config(&cfg);
            let became_authenticated = matches!(next, AuthState::Authenticated { .. });

            // Single emitter per D-13: transition() atomically updates the Mutex-held
            // AuthState AND emits `auth-state-changed` with the discriminated-union payload.
            auth::transition(&app_clone, &handle_clone, next);

            // Surface a one-shot toast: "Signed in via your terminal."
            if was_local_only && became_authenticated {
                let user_short = if cfg.user_id.len() >= 8 {
                    cfg.user_id[..8].to_string()
                } else {
                    cfg.user_id.clone()
                };
                if let Err(e) = (crate::events::AuthAdoptedFromCli { user_short }).emit(&app_clone)
                {
                    warn!("emit AuthAdoptedFromCli failed: {}", e);
                }
            }
        }
    });

    Ok(())
}

/// Pure helper — converts a Config snapshot into the canonical next AuthState.
/// Extracted so VALIDATION 2-02-04 can unit-test the classification logic without
/// spinning up the full FS watcher + Tauri harness.
pub(crate) fn next_state_from_config(cfg: &crate::protocol::Config) -> AuthState {
    if cfg.user_id.is_empty() || cfg.active_device_id.is_empty() {
        return AuthState::LocalOnly;
    }
    let active_relay_id = credential::load_multi_config()
        .ok()
        .and_then(|mc| mc.active_relay_id.clone())
        .unwrap_or_default();

    match credential::read_credentials(cfg) {
        Ok(_token) => AuthState::Authenticated {
            user_id: cfg.user_id.clone(),
            device_id: cfg.active_device_id.clone(),
            hostname: cfg.hostname.clone(),
            relay_url: cfg.relay_url.clone(),
            active_relay_id: active_relay_id.clone(),
            machine_id: client_core::machine::stable_machine_id(),
        },
        Err(_) => AuthState::LocalOnly,
    }
}

#[cfg(test)]
pub fn decide_emit(prev: u64, current: u64) -> bool {
    current > prev
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_emit_gates_on_version_bump() {
        assert!(decide_emit(0, 1));
        assert!(decide_emit(5, 6));
        assert!(!decide_emit(3, 3)); // idempotent — no fire
        assert!(!decide_emit(7, 5)); // stale — no fire
    }

    #[test]
    fn next_state_empty_config_is_local_only() {
        let cfg = crate::protocol::Config::default();
        match next_state_from_config(&cfg) {
            AuthState::LocalOnly => {}
            other => panic!("expected LocalOnly, got {:?}", other),
        }
    }

    #[test]
    fn next_state_plaintext_token_is_authenticated() {
        let cfg = crate::protocol::Config {
            user_id: "u1".into(),
            active_device_id: "d1".into(),
            token: "plaintext-token".into(),
            hostname: "laptop".into(),
            relay_url: "http://localhost:8080".into(),
            ..Default::default()
        };
        // read_credentials short-circuits on non-empty cfg.token (per credential.rs logic from Task 3).
        match next_state_from_config(&cfg) {
            AuthState::Authenticated {
                user_id,
                device_id,
                hostname,
                relay_url,
                ..
            } => {
                assert_eq!(user_id, "u1");
                assert_eq!(device_id, "d1");
                assert_eq!(hostname, "laptop");
                assert_eq!(relay_url, "http://localhost:8080");
            }
            other => panic!("expected Authenticated, got {:?}", other),
        }
    }
}
