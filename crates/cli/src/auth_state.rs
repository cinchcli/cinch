//! Shared "is this machine signed in?" gate used by the bare-`cinch`
//! welcome message and by the `push`/`pull` pre-flight checks.
//!
//! Returning `false` on any load error (corrupt config, missing file,
//! permission denied) is intentional: greeting a corrupt-config user
//! is harmless, while silently swallowing the message would be worse.

use client_core::config::MultiConfig;

use crate::exit::{ExitError, AUTH_FAILURE};

/// Pre-flight gate for commands that require an active token. Use at the
/// top of each command's `run()` to short-circuit before any network call
/// when the user hasn't signed in on this machine yet.
pub fn ensure_authenticated() -> Result<(), ExitError> {
    if is_authenticated() {
        return Ok(());
    }
    Err(ExitError::new(
        AUTH_FAILURE,
        "Not signed in on this machine.",
        "Run: cinch auth login",
    ))
}

/// True when the active relay profile has a non-empty token.
pub fn has_active_token(mc: &MultiConfig) -> bool {
    mc.active_profile()
        .map(|p| !p.token.is_empty())
        .unwrap_or(false)
}

/// Convenience: load the disk config and check `has_active_token`.
/// Disk errors fold to `false` (unauthenticated).
pub fn is_authenticated() -> bool {
    client_core::auth::load_multi_config()
        .map(|mc| has_active_token(&mc))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_core::config::{MultiConfig, RelayProfile};

    fn blank_profile(id: &str, token: &str) -> RelayProfile {
        RelayProfile {
            id: id.into(),
            label: String::new(),
            relay_url: String::new(),
            user_id: String::new(),
            device_id: String::new(),
            hostname: String::new(),
            encryption_key: String::new(),
            device_private_key: String::new(),
            credential_version: 0,
            token: token.into(),
            machine_id: String::new(),
            email: String::new(),
            identity_provider: String::new(),
            display_name: String::new(),
            key_pending: false,
        }
    }

    #[test]
    fn empty_multi_config_is_unauthenticated() {
        let mc = MultiConfig::default();
        assert!(!has_active_token(&mc));
    }

    #[test]
    fn profile_with_blank_token_is_unauthenticated() {
        let mc = MultiConfig {
            active_relay_id: Some("r1".into()),
            relays: vec![blank_profile("r1", "")],
        };
        assert!(!has_active_token(&mc));
    }

    #[test]
    fn profile_with_token_is_authenticated() {
        let mc = MultiConfig {
            active_relay_id: Some("r1".into()),
            relays: vec![blank_profile("r1", "abc")],
        };
        assert!(has_active_token(&mc));
    }

    #[test]
    fn ensure_authenticated_errors_when_no_token() {
        // Force is_authenticated() to return false by pointing HOME at a
        // tempdir with no .cinch/ subdirectory.
        let tmp = tempfile::tempdir().expect("tempdir");
        let _guard = EnvGuard::set("HOME", tmp.path());

        let err = ensure_authenticated().expect_err("should fail");
        assert_eq!(err.code, crate::exit::AUTH_FAILURE);
        assert!(
            err.fix.contains("cinch auth login"),
            "fix line missing hint: {}",
            err.fix
        );
    }

    /// Scoped env var override that restores the previous value on drop.
    /// Lives in this test module only; not exported.
    struct EnvGuard {
        key: &'static str,
        prev: Option<std::ffi::OsString>,
    }
    impl EnvGuard {
        fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
            let prev = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, prev }
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }
}
