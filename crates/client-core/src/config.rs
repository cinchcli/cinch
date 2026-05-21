//! On-disk client config: `~/.cinch/config.json`.
//!
//! Wire-compatible with the Go CLI's `cinch/internal/config/config.go` schema.
//! `MultiConfig` is the canonical disk format; `Config` is the legacy single-relay
//! shape kept for backwards compatibility (and as the in-memory shape consumers
//! handle most often via `MultiConfig::to_active_config`).
//!
//! Permissions: 0700 dir, 0600 file on Unix.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub user_id: String,
    #[serde(default = "default_relay_url")]
    pub relay_url: String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub active_device_id: String,
    #[serde(default)]
    pub credential_version: u64,
    #[serde(default)]
    pub encryption_key: String,
    #[serde(default)]
    pub device_private_key: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub identity_provider: String,
    #[serde(default)]
    pub display_name: String,
    /// True when this device registered its X25519 public key with the relay
    /// but has not yet received the user's master AES key via the
    /// `key_exchange_requested` ECDH flow. While true, `encryption_key` is
    /// empty and every push/pull must first attempt an auto-retry of
    /// `retry_key_bundle` + `poll_key_bundle`.
    ///
    /// Set by `auth_session::install_credentials` on a fresh sign-in, cleared
    /// by `auth::poll_key_bundle` the moment a master-key bundle arrives and
    /// decrypts.
    #[serde(default)]
    pub key_pending: bool,
}

pub fn default_relay_url() -> String {
    "http://localhost:8080".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayProfile {
    pub id: String,
    pub label: String,
    pub relay_url: String,
    pub user_id: String,
    pub device_id: String,
    pub hostname: String,
    #[serde(default)]
    pub encryption_key: String,
    #[serde(default)]
    pub device_private_key: String,
    #[serde(default)]
    pub credential_version: u64,
    #[serde(default)]
    pub token: String,
    /// Stable per-machine identifier (opaque hash). Used by the relay to
    /// recognize repeat sign-ins from the same machine and reuse a single
    /// device row instead of creating duplicates. Empty for legacy configs;
    /// backfilled on next save via `client_core::machine::stable_machine_id`.
    #[serde(default)]
    pub machine_id: String,
    /// Verified email address returned by the OAuth provider at login time.
    /// Empty for legacy configs or when the provider did not return a verified email.
    #[serde(default)]
    pub email: String,
    /// OAuth identity provider used for the most recent login ("google" or "github").
    /// Empty for legacy configs.
    #[serde(default)]
    pub identity_provider: String,
    /// Effective display name. Set by the user via `cinch auth set-name` or
    /// the desktop settings UI; falls back to the OAuth-fetched name on
    /// login if the user hasn't overridden it. Empty for legacy configs.
    #[serde(default)]
    pub display_name: String,
    /// Mirrors `Config::key_pending`. See that field's doc comment for the
    /// full semantics. Defaults to false so configs written by pre-fix
    /// builds — where a fresh AES key was always generated locally —
    /// continue to look "ready" on disk, and only newly-installed devices
    /// (after the fix lands) opt into the pending-key flow.
    #[serde(default)]
    pub key_pending: bool,
}

impl RelayProfile {
    pub fn from_config(cfg: &Config, label: Option<String>) -> Self {
        use ulid::Ulid;
        let id = Ulid::new().to_string();
        let label = label.unwrap_or_else(|| {
            url::Url::parse(&cfg.relay_url)
                .ok()
                .and_then(|u| u.host_str().map(|h| h.to_string()))
                .unwrap_or_else(|| cfg.relay_url.clone())
        });
        Self {
            id,
            label,
            relay_url: cfg.relay_url.clone(),
            user_id: cfg.user_id.clone(),
            device_id: cfg.active_device_id.clone(),
            hostname: cfg.hostname.clone(),
            encryption_key: cfg.encryption_key.clone(),
            device_private_key: cfg.device_private_key.clone(),
            credential_version: cfg.credential_version,
            token: cfg.token.clone(),
            machine_id: crate::machine::stable_machine_id(),
            email: cfg.email.clone(),
            identity_provider: cfg.identity_provider.clone(),
            display_name: cfg.display_name.clone(),
            key_pending: cfg.key_pending,
        }
    }

    pub fn to_config(&self) -> Config {
        Config {
            token: self.token.clone(),
            user_id: self.user_id.clone(),
            relay_url: self.relay_url.clone(),
            hostname: self.hostname.clone(),
            active_device_id: self.device_id.clone(),
            credential_version: self.credential_version,
            encryption_key: self.encryption_key.clone(),
            device_private_key: self.device_private_key.clone(),
            email: self.email.clone(),
            identity_provider: self.identity_provider.clone(),
            display_name: self.display_name.clone(),
            key_pending: self.key_pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MultiConfig {
    #[serde(default)]
    pub active_relay_id: Option<String>,
    #[serde(default)]
    pub relays: Vec<RelayProfile>,
}

pub type MultiConfigHandle = Arc<Mutex<MultiConfig>>;

impl MultiConfig {
    pub fn load() -> Self {
        let Some(home) = dirs::home_dir() else {
            return Self::default();
        };
        let path = home.join(".cinch").join("config.json");
        if !path.exists() {
            return Self::default();
        }
        let Ok(data) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&data) else {
            return Self::default();
        };
        if v.get("relays").is_some() {
            serde_json::from_value(v).unwrap_or_default()
        } else {
            let old: Config = match serde_json::from_value(v) {
                Ok(c) => c,
                Err(_) => return Self::default(),
            };
            Self::from_legacy(old)
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let home = dirs::home_dir().ok_or("cannot determine home directory")?;
        let dir = home.join(".cinch");
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {}", e))?;
        let path = dir.join("config.json");
        let data = serde_json::to_string_pretty(self).map_err(|e| format!("marshal: {}", e))?;
        std::fs::write(&path, &data).map_err(|e| format!("write: {}", e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(&path) {
                let mut perms = meta.permissions();
                perms.set_mode(0o600);
                let _ = std::fs::set_permissions(&path, perms);
            }
        }
        Ok(())
    }

    pub fn active_profile(&self) -> Option<&RelayProfile> {
        let id = self.active_relay_id.as_deref()?;
        self.relays.iter().find(|r| r.id == id)
    }

    pub fn active_profile_mut(&mut self) -> Option<&mut RelayProfile> {
        let id = self.active_relay_id.clone()?;
        self.relays.iter_mut().find(|r| r.id == id)
    }

    pub fn to_active_config(&self) -> Config {
        self.active_profile()
            .map(|p| p.to_config())
            .unwrap_or_default()
    }

    pub fn from_legacy_pub(old: Config) -> Self {
        Self::from_legacy(old)
    }

    fn from_legacy(old: Config) -> Self {
        if old.user_id.is_empty() && old.token.is_empty() {
            return Self::default();
        }
        let profile = RelayProfile::from_config(&old, None);
        let id = profile.id.clone();
        Self {
            active_relay_id: Some(id),
            relays: vec![profile],
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            token: String::new(),
            user_id: String::new(),
            relay_url: default_relay_url(),
            hostname: String::new(),
            active_device_id: String::new(),
            credential_version: 0,
            encryption_key: String::new(),
            device_private_key: String::new(),
            email: String::new(),
            identity_provider: String::new(),
            display_name: String::new(),
            key_pending: false,
        }
    }
}

impl Config {
    pub fn is_configured(&self) -> bool {
        !self.user_id.is_empty() && !self.active_device_id.is_empty()
    }

    pub fn load() -> Result<Self, String> {
        let mc = MultiConfig::load();
        let cfg = mc.to_active_config();
        if cfg.user_id.is_empty() && cfg.token.is_empty() {
            return Err("no active relay configured — run: cinch auth login".to_string());
        }
        Ok(cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_configured_accepts_keyring_backed_config() {
        let config = Config {
            token: String::new(),
            user_id: "u1".into(),
            relay_url: "https://api.cinchcli.com".into(),
            hostname: "macbook".into(),
            active_device_id: "d1".into(),
            credential_version: 1,
            encryption_key: String::new(),
            device_private_key: String::new(),
            email: String::new(),
            identity_provider: String::new(),
            display_name: String::new(),
            key_pending: false,
        };
        assert!(config.is_configured());
    }

    #[test]
    fn relay_profile_roundtrips_display_name_through_config() {
        let cfg = Config {
            token: "t".into(),
            user_id: "u".into(),
            relay_url: "https://r".into(),
            hostname: "h".into(),
            active_device_id: "d".into(),
            credential_version: 1,
            encryption_key: String::new(),
            device_private_key: String::new(),
            email: "alice@example.com".into(),
            identity_provider: "github".into(),
            display_name: "Alice Example".into(),
            key_pending: false,
        };
        let prof = RelayProfile::from_config(&cfg, Some("test".into()));
        assert_eq!(prof.display_name, "Alice Example");
        let back = prof.to_config();
        assert_eq!(back.display_name, "Alice Example");
    }

    #[test]
    fn relay_profile_defaults_display_name_when_legacy_json_missing_field() {
        let json = r#"{
            "id": "01HZ",
            "label": "main",
            "relay_url": "https://r",
            "user_id": "u",
            "device_id": "d",
            "hostname": "h"
        }"#;
        let p: RelayProfile = serde_json::from_str(json).expect("decode");
        assert_eq!(p.display_name, "");
    }
}
