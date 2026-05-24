#![allow(dead_code)]
//! AuthState sum type — the single source of truth for desktop auth UI.
//!
//! Owned by Rust (Arc<Mutex<AuthState>>), subscribed via Tauri event `auth-state-changed`.
//! Variants locked by CONTEXT.md D-12; transition triggers by D-15; backoff by D-16.
//!
//! transition() is the SINGLE auth-state-changed emitter per CONTEXT.md D-13.
//! FS watcher, WS handlers, and Tauri commands MUST funnel through it.
//! No direct `app.emit("auth-state-changed", ...)` is permitted anywhere else in the codebase.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

pub type AuthStateHandle = Arc<Mutex<AuthState>>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Type)]
#[serde(tag = "variant", content = "payload")]
pub enum AuthState {
    #[default]
    LocalOnly,
    Authenticating {
        progress: AuthProgress,
    },
    Authenticated {
        user_id: String,
        device_id: String,
        hostname: String,
        relay_url: String,
        active_relay_id: String,
        machine_id: String,
    },
    ErrorRecoverable {
        reason: AuthErrorReason,
        retry_after_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
#[serde(tag = "kind")]
pub enum AuthProgress {
    SigningIn,
    Pairing,
    RotatingToken,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
#[serde(tag = "kind")]
pub enum AuthErrorReason {
    RelayUnreachable,
    KeyringUnavailable,
    NetworkDown,
    InvalidPairToken,
}

/// transition is THE ONLY function that emits `auth-state-changed`.
/// Any component (FS watcher, WS handler, Tauri command) that wants to change AuthState
/// MUST call this function — never `app.emit("auth-state-changed", ...)` directly.
///
/// Critical invariant (RESEARCH Pitfall 1 — also covered by T-2-20 in Plan 03 threat model):
/// the MutexGuard is dropped BEFORE the emit call so subscribers cannot deadlock on re-entry.
pub fn transition(app: &AppHandle, handle: &AuthStateHandle, next: AuthState) {
    let snapshot = next.clone();
    {
        let mut guard = handle.lock().unwrap();
        *guard = next;
    } // guard dropped here
    let tag = snapshot_tag(&snapshot);
    let snapshot_for_tray = snapshot.clone();
    match crate::events::AuthStateChanged(snapshot).emit(app) {
        Ok(()) => log::info!("auth-state: {:?}", tag),
        Err(e) => log::warn!("auth-state-changed emit failed: {}", e),
    }
    let ws_value: String = match app.try_state::<std::sync::Arc<crate::sync_status::WsStatus>>() {
        Some(ws) => ws.get(),
        None => String::new(),
    };
    crate::tray::set_status(app, &snapshot_for_tray, &ws_value);
}

fn snapshot_tag(s: &AuthState) -> &'static str {
    match s {
        AuthState::LocalOnly => "LocalOnly",
        AuthState::Authenticating { .. } => "Authenticating",
        AuthState::Authenticated { .. } => "Authenticated",
        AuthState::ErrorRecoverable { .. } => "ErrorRecoverable",
    }
}

/// Events that drive AuthState transitions. One variant per row of D-15's table.
#[derive(Debug)]
pub enum AuthEvent {
    AppStart {
        creds_present: bool,
    },
    ClickSignIn,
    PairStart,
    PairSuccess {
        user_id: String,
        device_id: String,
        hostname: String,
        relay_url: String,
        active_relay_id: String,
        machine_id: String,
    },
    PairFailure {
        kind: PairFailureKind,
    },
    WsTokenRotated,
    Ws401 {
        reason: Ws401Reason,
    },
    WsDisconnect,
    Relay5xx,
    KeyringUnlockFailure, // transient — ErrorRecoverable
    KeyringEntryMissing,  // permanent — LocalOnly
    FsConfigVersionBump,
    FsConfigDeleted,
    Logout,
}

#[derive(Debug)]
pub enum PairFailureKind {
    Network,
    BadPairToken,
}

#[derive(Debug)]
pub enum Ws401Reason {
    InvalidToken,
    DeviceRevoked,
}

/// classify_next_state is the PURE transition function — same inputs produce same outputs.
/// Every row of D-15 maps to a match arm here. Side effects (keyring writes, toasts, FS reads)
/// happen in callers; this fn is the state-machine brain.
pub fn classify_next_state(current: &AuthState, event: &AuthEvent) -> AuthState {
    match event {
        AuthEvent::AppStart {
            creds_present: false,
        } => AuthState::LocalOnly,
        AuthEvent::AppStart {
            creds_present: true,
        } => AuthState::Authenticated {
            user_id: String::new(),
            device_id: String::new(),
            hostname: String::new(),
            relay_url: String::new(),
            active_relay_id: String::new(),
            machine_id: String::new(),
        }, // caller fills in payload from Config::load (optimistic — reverts on WS 401 per D-15)

        AuthEvent::ClickSignIn => AuthState::Authenticating {
            progress: AuthProgress::SigningIn,
        },
        AuthEvent::PairStart => AuthState::Authenticating {
            progress: AuthProgress::Pairing,
        },

        AuthEvent::PairSuccess {
            user_id,
            device_id,
            hostname,
            relay_url,
            active_relay_id,
            machine_id,
        } => AuthState::Authenticated {
            user_id: user_id.clone(),
            device_id: device_id.clone(),
            hostname: hostname.clone(),
            relay_url: relay_url.clone(),
            active_relay_id: active_relay_id.clone(),
            machine_id: machine_id.clone(),
        },

        AuthEvent::PairFailure {
            kind: PairFailureKind::Network,
        } => AuthState::ErrorRecoverable {
            reason: AuthErrorReason::RelayUnreachable,
            retry_after_ms: Some(5_000),
        },
        AuthEvent::PairFailure {
            kind: PairFailureKind::BadPairToken,
        } => AuthState::LocalOnly,

        AuthEvent::WsTokenRotated => AuthState::Authenticating {
            progress: AuthProgress::RotatingToken,
        },

        AuthEvent::Ws401 {
            reason: Ws401Reason::InvalidToken,
        } => AuthState::LocalOnly,
        AuthEvent::Ws401 {
            reason: Ws401Reason::DeviceRevoked,
        } => AuthState::LocalOnly,

        AuthEvent::WsDisconnect => current.clone(), // stays as-is (WsStatus separately tracks network)

        AuthEvent::Relay5xx => AuthState::ErrorRecoverable {
            reason: AuthErrorReason::RelayUnreachable,
            retry_after_ms: Some(5_000),
        },

        AuthEvent::KeyringUnlockFailure => AuthState::ErrorRecoverable {
            reason: AuthErrorReason::KeyringUnavailable,
            retry_after_ms: Some(2_000),
        },
        AuthEvent::KeyringEntryMissing => AuthState::LocalOnly,

        AuthEvent::FsConfigVersionBump => current.clone(), // caller re-reads and calls transition() with the new payload
        AuthEvent::FsConfigDeleted => AuthState::LocalOnly,

        AuthEvent::Logout => AuthState::LocalOnly,
    }
}

// ---------------------------------------------------------------------------
// Pending device-code list (remote-login approval flow)
// ---------------------------------------------------------------------------

/// A device-code approval request forwarded from the relay via WebSocket.
/// Derives `specta::Type` so tauri-specta can generate the TypeScript binding.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct PendingDeviceCode {
    pub user_code: String,
    pub hostname: String,
    pub source_region: String,
    /// Unix timestamp (seconds) when the request arrived at the relay.
    pub requested_at: i64,
}

#[cfg(test)]
impl PendingDeviceCode {
    fn sample(code: &str) -> Self {
        Self {
            user_code: code.into(),
            hostname: "dev-box-3".into(),
            source_region: "us-west".into(),
            requested_at: chrono::Utc::now().timestamp(),
        }
    }
}

/// Shared in-memory list of pending device-code approval requests.
/// Follows the same `Arc<Mutex<...>>` type-alias pattern as `AuthStateHandle`.
pub type PendingCodesHandle = Arc<Mutex<Vec<PendingDeviceCode>>>;

/// Push `p` onto the list, silently deduplicating by `user_code`.
pub fn add_pending_code(handle: &PendingCodesHandle, p: PendingDeviceCode) {
    let mut guard = handle.lock().unwrap();
    if guard
        .iter()
        .any(|existing| existing.user_code == p.user_code)
    {
        return;
    }
    guard.push(p);
}

/// Remove the entry whose `user_code` matches `code`. No-op if not found.
pub fn remove_pending_code(handle: &PendingCodesHandle, code: &str) {
    let mut guard = handle.lock().unwrap();
    guard.retain(|p| p.user_code != code);
}

/// Return the number of pending requests.
pub fn pending_count(handle: &PendingCodesHandle) -> usize {
    handle.lock().unwrap().len()
}

/// Return a cloned snapshot of all pending requests.
pub fn pending_codes(handle: &PendingCodesHandle) -> Vec<PendingDeviceCode> {
    handle.lock().unwrap().clone()
}

/// Drop entries whose `requested_at` is older than `ttl` from now.
pub fn sweep_expired(handle: &PendingCodesHandle, ttl: std::time::Duration) {
    let ttl_secs = ttl.as_secs() as i64;
    let now = chrono::Utc::now().timestamp();
    let cutoff = now - ttl_secs;
    let mut guard = handle.lock().unwrap();
    guard.retain(|p| p.requested_at >= cutoff);
}

// ---------------------------------------------------------------------------

/// Exponential backoff per D-16: 5s initial, 2x growth, 60s cap. Reset to 5s on success.
/// Used for both the ws.rs reconnect loop and ErrorRecoverable retry_after_ms calculations.
pub struct Backoff {
    current_ms: u64,
}

impl Backoff {
    pub fn new() -> Self {
        Self { current_ms: 5_000 }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Duration {
        let d = Duration::from_millis(self.current_ms);
        self.current_ms = (self.current_ms * 2).min(60_000);
        d
    }

    pub fn reset(&mut self) {
        self.current_ms = 5_000;
    }

    pub fn current_ms(&self) -> u64 {
        self.current_ms
    }
}

impl Default for Backoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_until_cap() {
        let mut b = Backoff::new();
        assert_eq!(b.next(), Duration::from_millis(5_000));
        assert_eq!(b.next(), Duration::from_millis(10_000));
        assert_eq!(b.next(), Duration::from_millis(20_000));
        assert_eq!(b.next(), Duration::from_millis(40_000));
        assert_eq!(b.next(), Duration::from_millis(60_000)); // cap
        assert_eq!(b.next(), Duration::from_millis(60_000)); // stays capped
        b.reset();
        assert_eq!(b.next(), Duration::from_millis(5_000));
    }

    // Table-driven tests covering D-15's 13-row transition table.
    // Each test asserts the expected next-state given a prior state + event.

    fn u(s: &str) -> String {
        s.to_string()
    }

    fn sample_authed() -> AuthState {
        AuthState::Authenticated {
            user_id: u("u_123"),
            device_id: u("d_456"),
            hostname: u("macbook"),
            relay_url: u("http://localhost:8080"),
            active_relay_id: u("relay_01"),
            machine_id: u(""),
        }
    }

    #[test]
    fn d15_app_start_no_creds() {
        assert_eq!(
            classify_next_state(
                &AuthState::LocalOnly,
                &AuthEvent::AppStart {
                    creds_present: false
                }
            ),
            AuthState::LocalOnly
        );
    }

    #[test]
    fn d15_app_start_with_creds() {
        let next = classify_next_state(
            &AuthState::LocalOnly,
            &AuthEvent::AppStart {
                creds_present: true,
            },
        );
        match next {
            AuthState::Authenticated { .. } => {}
            other => panic!("expected Authenticated, got {:?}", other),
        }
    }

    #[test]
    fn d15_click_signin_from_local() {
        assert!(matches!(
            classify_next_state(&AuthState::LocalOnly, &AuthEvent::ClickSignIn),
            AuthState::Authenticating {
                progress: AuthProgress::SigningIn
            }
        ));
    }

    #[test]
    fn d15_pair_start() {
        assert!(matches!(
            classify_next_state(&sample_authed(), &AuthEvent::PairStart),
            AuthState::Authenticating {
                progress: AuthProgress::Pairing
            }
        ));
    }

    #[test]
    fn d15_pair_succeeds_to_authed() {
        assert!(matches!(
            classify_next_state(
                &AuthState::Authenticating {
                    progress: AuthProgress::Pairing
                },
                &AuthEvent::PairSuccess {
                    user_id: u("u_1"),
                    device_id: u("d_2"),
                    hostname: u("h"),
                    relay_url: u("r"),
                    active_relay_id: u("relay_01"),
                    machine_id: u(""),
                },
            ),
            AuthState::Authenticated { .. }
        ));
    }

    #[test]
    fn d15_pair_network_failure() {
        let next = classify_next_state(
            &AuthState::Authenticating {
                progress: AuthProgress::Pairing,
            },
            &AuthEvent::PairFailure {
                kind: PairFailureKind::Network,
            },
        );
        match next {
            AuthState::ErrorRecoverable {
                reason: AuthErrorReason::RelayUnreachable,
                retry_after_ms: Some(5_000),
            } => {}
            other => panic!(
                "expected ErrorRecoverable{{RelayUnreachable,5s}}, got {:?}",
                other
            ),
        }
    }

    #[test]
    fn d15_pair_bad_token_to_local() {
        assert_eq!(
            classify_next_state(
                &AuthState::Authenticating {
                    progress: AuthProgress::Pairing
                },
                &AuthEvent::PairFailure {
                    kind: PairFailureKind::BadPairToken
                },
            ),
            AuthState::LocalOnly
        );
    }

    #[test]
    fn d15_token_rotated() {
        assert!(matches!(
            classify_next_state(&sample_authed(), &AuthEvent::WsTokenRotated),
            AuthState::Authenticating {
                progress: AuthProgress::RotatingToken
            }
        ));
    }

    #[test]
    fn d15_ws_401_invalid_token() {
        assert_eq!(
            classify_next_state(
                &sample_authed(),
                &AuthEvent::Ws401 {
                    reason: Ws401Reason::InvalidToken
                }
            ),
            AuthState::LocalOnly
        );
    }

    #[test]
    fn d15_ws_401_device_revoked() {
        assert_eq!(
            classify_next_state(
                &sample_authed(),
                &AuthEvent::Ws401 {
                    reason: Ws401Reason::DeviceRevoked
                }
            ),
            AuthState::LocalOnly
        );
    }

    #[test]
    fn d15_ws_disconnect_stays_authed() {
        assert!(matches!(
            classify_next_state(&sample_authed(), &AuthEvent::WsDisconnect),
            AuthState::Authenticated { .. }
        ));
    }

    #[test]
    fn d15_relay_5xx_to_error_recoverable() {
        let next = classify_next_state(&sample_authed(), &AuthEvent::Relay5xx);
        match next {
            AuthState::ErrorRecoverable {
                reason: AuthErrorReason::RelayUnreachable,
                retry_after_ms: Some(5_000),
            } => {}
            other => panic!("expected ErrorRecoverable, got {:?}", other),
        }
    }

    #[test]
    fn d15_keyring_missing_to_local() {
        assert_eq!(
            classify_next_state(&sample_authed(), &AuthEvent::KeyringEntryMissing),
            AuthState::LocalOnly
        );
    }

    #[test]
    fn d15_logout_to_local() {
        assert_eq!(
            classify_next_state(&sample_authed(), &AuthEvent::Logout),
            AuthState::LocalOnly
        );
    }

    // --- PendingCodesHandle tests ---

    #[test]
    fn add_and_remove() {
        let h: PendingCodesHandle = Arc::new(Mutex::new(Vec::new()));
        add_pending_code(&h, PendingDeviceCode::sample("ABCD-1234"));
        assert_eq!(pending_count(&h), 1);
        remove_pending_code(&h, "ABCD-1234");
        assert_eq!(pending_count(&h), 0);
    }

    #[test]
    fn dedup_same_user_code() {
        let h: PendingCodesHandle = Arc::new(Mutex::new(Vec::new()));
        add_pending_code(&h, PendingDeviceCode::sample("ABCD-1234"));
        add_pending_code(&h, PendingDeviceCode::sample("ABCD-1234"));
        assert_eq!(pending_count(&h), 1);
    }

    #[test]
    fn ttl_sweep_drops_expired() {
        let h: PendingCodesHandle = Arc::new(Mutex::new(Vec::new()));
        let mut p = PendingDeviceCode::sample("ABCD-1234");
        p.requested_at = chrono::Utc::now().timestamp() - 6 * 60; // 6 min ago
        add_pending_code(&h, p);
        sweep_expired(&h, std::time::Duration::from_secs(5 * 60));
        assert_eq!(pending_count(&h), 0);
    }
}
