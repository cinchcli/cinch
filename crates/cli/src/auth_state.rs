//! Shared "is this machine signed in?" gate used by the bare-`cinch`
//! welcome message and by the `push`/`pull` pre-flight checks.
//!
//! Returning `false` on any load error (corrupt config, missing file,
//! permission denied) is intentional: greeting a corrupt-config user
//! is harmless, while silently swallowing the message would be worse.

use client_core::config::MultiConfig;

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
}
