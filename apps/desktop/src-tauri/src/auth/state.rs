//! AuthState sum type — the single source of truth for desktop auth UI.
//!
//! Owned by Rust (Arc<Mutex<AuthState>>), subscribed via Tauri event `auth-state-changed`.
//! Variants locked by CONTEXT.md D-12; transition triggers by D-15; backoff by D-16.
//!
//! transition() is the SINGLE auth-state-changed emitter per CONTEXT.md D-13.
//! FS watcher, WS handlers, and Tauri commands MUST funnel through it.
//! No direct `app.emit("auth-state-changed", ...)` is permitted anywhere else in the codebase.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_specta::Event;

// Re-export the sibling modules' types so existing `crate::auth::state::*`
// call sites (events.rs, lib.rs, commands/auth/remote_login.rs, tests) keep
// resolving after the split into focused modules.
pub use super::backoff::Backoff;
pub use super::pending_codes::{
    add_pending_code, pending_codes, pending_count, remove_pending_code, sweep_expired,
    PendingCodesHandle, PendingDeviceCode,
};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
