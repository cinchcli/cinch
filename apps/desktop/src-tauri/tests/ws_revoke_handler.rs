//! AUTH-05 + AUTH-04: WS 401 device_revoked dispatch produces LocalOnly.
//!
//! These tests exercise the pure `classify_next_state` function from auth::state
//! to confirm that WS-level revoke events correctly drive the AuthState machine
//! to LocalOnly — satisfying VALIDATION 2-03-04.

use cinch_desktop_lib::auth::{classify_next_state, AuthEvent, AuthState, Ws401Reason};

#[test]
fn ws_revoked_transitions_to_local_only() {
    let current = AuthState::Authenticated {
        user_id: "u".into(),
        device_id: "d".into(),
        hostname: "h".into(),
        relay_url: "r".into(),
        active_relay_id: String::new(),
        machine_id: String::new(),
    };
    let next = classify_next_state(
        &current,
        &AuthEvent::Ws401 {
            reason: Ws401Reason::DeviceRevoked,
        },
    );
    assert_eq!(next, AuthState::LocalOnly);
}

#[test]
fn ws_invalid_token_transitions_to_local_only() {
    let current = AuthState::Authenticated {
        user_id: "u".into(),
        device_id: "d".into(),
        hostname: "h".into(),
        relay_url: "r".into(),
        active_relay_id: String::new(),
        machine_id: String::new(),
    };
    let next = classify_next_state(
        &current,
        &AuthEvent::Ws401 {
            reason: Ws401Reason::InvalidToken,
        },
    );
    assert_eq!(next, AuthState::LocalOnly);
}
