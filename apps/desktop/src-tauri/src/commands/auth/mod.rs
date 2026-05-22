//! Tauri commands for auth state — called from React via invoke().
//! All commands return Result<T, String> to match the existing convention.

use tauri::{AppHandle, State};

use crate::auth::{transition, AuthState, AuthStateHandle};

mod deeplink;
mod display_name;
mod providers;
mod remote_login;
mod sign_in;
mod sign_out;
mod ssh_pair;

pub use deeplink::*;
pub use display_name::*;
pub use providers::*;
pub use remote_login::*;
pub use sign_in::*;
pub use sign_out::*;
pub use ssh_pair::*;

/// Display-facing user profile sourced from the active relay profile.
///
/// Returned by [`get_user_profile`]. All fields are empty strings when no
/// relay profile is configured (graceful pre-auth state).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct UserProfile {
    pub display_name: String,
    pub email: String,
    pub identity_provider: String,
    pub user_id: String,
}

/// Returns display-facing identity fields from the active relay profile.
///
/// Falls back to all-empty-string `UserProfile` when no profile is configured
/// (e.g. before first sign-in) so the frontend can render a sensible
/// unauthenticated state without a `Result` branch.
#[tauri::command]
#[specta::specta]
pub fn get_user_profile() -> UserProfile {
    let cfg = match crate::protocol::Config::load() {
        Ok(c) => c,
        Err(_) => {
            return UserProfile {
                display_name: String::new(),
                email: String::new(),
                identity_provider: String::new(),
                user_id: String::new(),
            }
        }
    };
    UserProfile {
        display_name: cfg.display_name,
        email: cfg.email,
        identity_provider: cfg.identity_provider,
        user_id: cfg.user_id,
    }
}

/// Returns the current AuthState. Used by AuthProvider's initial fetch in React.
#[tauri::command]
#[specta::specta]
pub fn get_auth_state(handle: State<'_, AuthStateHandle>) -> AuthState {
    handle.lock().unwrap().clone()
}

/// retry_auth — bypasses ErrorRecoverable.retry_after_ms and re-attempts the last failing operation.
/// For Phase 2 plumbing: resets to LocalOnly and lets the user re-invoke sign_in.
/// Phase 3+ will store the last-attempted operation and retry it in place.
#[tauri::command]
#[specta::specta]
pub async fn retry_auth(app: AppHandle, handle: State<'_, AuthStateHandle>) -> Result<(), String> {
    // Conservative v1: just transition to LocalOnly; React re-renders SetupScreen.
    transition(&app, &handle, AuthState::LocalOnly);
    Ok(())
}

#[cfg(test)]
mod user_profile_tests {
    use super::UserProfile;

    #[test]
    fn user_profile_struct_serializes_with_expected_keys() {
        let p = UserProfile {
            display_name: "Alice Example".into(),
            email: "alice@example.com".into(),
            identity_provider: "github".into(),
            user_id: "01HZ".into(),
        };
        let v: serde_json::Value = serde_json::to_value(&p).unwrap();
        assert_eq!(v["display_name"], "Alice Example");
        assert_eq!(v["email"], "alice@example.com");
        assert_eq!(v["identity_provider"], "github");
        assert_eq!(v["user_id"], "01HZ");
    }
}
