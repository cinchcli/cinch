#![allow(dead_code, unused_imports)]

pub mod credential;
pub mod propagate;
pub mod state; // Plan 02 Task 3 seeded the skeleton; Plan 03 Task 1 extends with events + classify_next_state + Backoff + tests

pub use credential::{
    add_relay_profile, load_multi_config, read_credentials, rotate_credentials, save_multi_config,
    wipe_credentials, wipe_relay_credentials, write_credentials, CredentialError,
};

// Re-export AuthState primitives so downstream modules (propagate.rs, ws.rs, commands/auth.rs)
// can reach `crate::auth::transition(...)` and `crate::auth::AuthStateHandle` without going
// through `state::` explicitly.
// BLOCKER 1 remediation — makes transition() the single `auth-state-changed` emitter.
pub use state::{
    classify_next_state, transition, AuthErrorReason, AuthEvent, AuthProgress, AuthState,
    AuthStateHandle, Backoff, PairFailureKind, Ws401Reason,
};

pub use propagate::spawn_credential_watcher;
