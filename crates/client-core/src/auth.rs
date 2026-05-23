#![allow(dead_code)]
//! Credential storage — `~/.cinch/config.json` (0600 permissions).
//!
//! This module is the source of truth for the disk credential format used
//! by both CLI and desktop. The Go CLI's
//! `cinch/cmd/internal/credstore/store.go` uses identical service/account
//! conventions; do not change `SERVICE_NAME` or the account key format
//! without coordinated updates on both sides.

use std::fs;
use std::path::PathBuf;

use crate::config::{Config, MultiConfig, RelayProfile};

pub const SERVICE_NAME: &str = "com.cinchcli";

/// Legacy Keychain service name used by builds prior to 2026-04-29. The
/// credstore reads this as a fallback and migrates entries forward on
/// first successful read.
pub const LEGACY_SERVICE_NAME: &str = "com.cinch.app";

#[derive(Debug)]
pub enum CredentialError {
    NoEntry,
    Io(String),
    BadConfig(String),
}

impl std::fmt::Display for CredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CredentialError::NoEntry => write!(f, "no credential stored"),
            CredentialError::Io(s) => write!(f, "io: {}", s),
            CredentialError::BadConfig(s) => write!(f, "bad config: {}", s),
        }
    }
}

fn account_key(user_id: &str, device_id: &str) -> String {
    format!("{}:{}", user_id, device_id)
}

fn config_path() -> Result<PathBuf, CredentialError> {
    let home = dirs::home_dir()
        .ok_or_else(|| CredentialError::Io("cannot determine home directory".into()))?;
    Ok(home.join(".cinch").join("config.json"))
}

pub fn load_multi_config() -> Result<MultiConfig, CredentialError> {
    let p = config_path()?;
    if !p.exists() {
        return Ok(MultiConfig::default());
    }
    let data =
        fs::read_to_string(&p).map_err(|e| CredentialError::Io(format!("read config: {}", e)))?;
    let v: serde_json::Value = serde_json::from_str(&data)
        .map_err(|e| CredentialError::BadConfig(format!("parse config: {}", e)))?;
    if v.get("relays").is_some() {
        serde_json::from_value(v)
            .map_err(|e| CredentialError::BadConfig(format!("parse multi_config: {}", e)))
    } else {
        let old: Config = serde_json::from_value(v)
            .map_err(|e| CredentialError::BadConfig(format!("parse legacy config: {}", e)))?;
        Ok(MultiConfig::from_legacy_pub(old))
    }
}

pub fn save_multi_config(mc: &MultiConfig) -> Result<(), CredentialError> {
    let p = config_path()?;
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| CredentialError::Io(format!("mkdir: {}", e)))?;
    }
    let data = serde_json::to_string_pretty(mc)
        .map_err(|e| CredentialError::BadConfig(format!("marshal: {}", e)))?;
    fs::write(&p, data).map_err(|e| CredentialError::Io(format!("write config: {}", e)))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&p)
            .map_err(|e| CredentialError::Io(format!("stat: {}", e)))?
            .permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&p, perms)
            .map_err(|e| CredentialError::Io(format!("chmod 0600: {}", e)))?;
    }
    Ok(())
}

pub fn load_config() -> Result<Config, CredentialError> {
    Ok(load_multi_config()?.to_active_config())
}

pub fn save_config_to_disk(cfg: &Config) -> Result<(), CredentialError> {
    let mut mc = load_multi_config()?;
    if let Some(profile) = mc.active_profile_mut() {
        profile.token = cfg.token.clone();
        profile.user_id = cfg.user_id.clone();
        profile.relay_url = cfg.relay_url.clone();
        profile.hostname = cfg.hostname.clone();
        profile.device_id = cfg.active_device_id.clone();
        profile.credential_version = cfg.credential_version;
        profile.encryption_key = cfg.encryption_key.clone();
        profile.device_private_key = cfg.device_private_key.clone();
        profile.key_pending = cfg.key_pending;
        if profile.machine_id.is_empty() {
            profile.machine_id = crate::machine::stable_machine_id();
        }
    } else {
        let profile = RelayProfile::from_config(cfg, None);
        let id = profile.id.clone();
        mc.relays.push(profile);
        mc.active_relay_id = Some(id);
    }
    save_multi_config(&mc)
}

/// Add a new RelayProfile to MultiConfig for a freshly-authenticated relay.
/// Used by the deep-link callback when PendingRelayAdd is set.
/// Returns relay_id.
pub fn add_relay_profile(
    user_id: &str,
    device_id: &str,
    token: &str,
    relay_url: &str,
    hostname: &str,
    label: Option<&str>,
    device_private_key: &str,
) -> Result<String, CredentialError> {
    let mut mc = load_multi_config()?;

    let label_str = label
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            url::Url::parse(relay_url)
                .ok()
                .and_then(|u| u.host_str().map(|h| h.to_string()))
                .unwrap_or_else(|| relay_url.to_string())
        });

    let next_version = mc
        .relays
        .iter()
        .map(|r| r.credential_version)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| CredentialError::BadConfig("credential_version overflow".into()))?;

    use ulid::Ulid;
    let relay_id = Ulid::new().to_string();
    let profile = RelayProfile {
        id: relay_id.clone(),
        label: label_str,
        relay_url: relay_url.to_string(),
        user_id: user_id.to_string(),
        device_id: device_id.to_string(),
        hostname: hostname.to_string(),
        encryption_key: String::new(),
        device_private_key: device_private_key.to_string(),
        credential_version: next_version,
        token: token.to_string(),
        machine_id: crate::machine::stable_machine_id(),
        email: String::new(),
        identity_provider: String::new(),
        display_name: String::new(),
        // The deep-link relay-add path has no master key yet; it will be
        // populated by the subsequent key-exchange handshake. Mark pending
        // so push/pull know to auto-retry until the bundle arrives.
        key_pending: true,
    };
    mc.relays.push(profile);
    if mc.active_relay_id.is_none() {
        mc.active_relay_id = Some(relay_id.clone());
    }
    save_multi_config(&mc)?;
    Ok(relay_id)
}

/// Remove credentials for a specific relay from MultiConfig.
pub fn wipe_relay_credentials(relay_id: &str) -> Result<(), CredentialError> {
    let mut mc = load_multi_config()?;
    mc.relays.retain(|r| r.id != relay_id);
    if mc.active_relay_id.as_deref() == Some(relay_id) {
        mc.active_relay_id = mc.relays.first().map(|r| r.id.clone());
    }
    let new_version = mc
        .relays
        .iter()
        .map(|r| r.credential_version)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| CredentialError::BadConfig("credential_version overflow".into()))?;
    if let Some(p) = mc.active_profile_mut() {
        p.credential_version = new_version;
    }
    save_multi_config(&mc)
}

/// write_credentials stores token in config.json (0600).
/// Bumps credential_version and persists via save_config.
pub fn write_credentials(
    user_id: &str,
    device_id: &str,
    token: &str,
    relay_url: &str,
    hostname: &str,
) -> Result<(), CredentialError> {
    let mut cfg = load_config()?;
    cfg.token = token.to_string();
    cfg.user_id = user_id.to_string();
    cfg.active_device_id = device_id.to_string();
    cfg.relay_url = relay_url.to_string();
    cfg.hostname = hostname.to_string();
    cfg.credential_version = cfg
        .credential_version
        .checked_add(1)
        .ok_or_else(|| CredentialError::BadConfig("credential_version overflow".into()))?;
    save_config_to_disk(&cfg)?;
    Ok(())
}

/// read_credentials returns the token for the currently-configured (user_id, device_id).
pub fn read_credentials(cfg: &Config) -> Result<String, CredentialError> {
    if cfg.user_id.is_empty() || cfg.active_device_id.is_empty() {
        return Err(CredentialError::NoEntry);
    }
    if cfg.token.is_empty() {
        return Err(CredentialError::NoEntry);
    }
    Ok(cfg.token.clone())
}

/// wipe_credentials clears all credential fields from config, bumps
/// credential_version, and best-effort deletes any Keychain entries left over
/// from pre-2026-05-08 CLI builds.
pub fn wipe_credentials() -> Result<(), CredentialError> {
    let mut cfg = load_config()?;
    let user_id = std::mem::take(&mut cfg.user_id);
    let device_id = std::mem::take(&mut cfg.active_device_id);
    cfg.token = String::new();
    cfg.encryption_key = String::new();
    cfg.device_private_key = String::new();
    cfg.credential_version = cfg
        .credential_version
        .checked_add(1)
        .ok_or_else(|| CredentialError::BadConfig("credential_version overflow".into()))?;
    save_config_to_disk(&cfg)?;
    crate::credstore::wipe_keyring_for(&user_id, &device_id);
    Ok(())
}

/// Read the encryption key for a user from config.
pub fn read_encryption_key(user_id: &str) -> Result<Vec<u8>, CredentialError> {
    if user_id.is_empty() {
        return Err(CredentialError::NoEntry);
    }
    let cfg = load_config()?;
    if !cfg.encryption_key.is_empty() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        if let Ok(key_bytes) = URL_SAFE_NO_PAD.decode(&cfg.encryption_key) {
            if key_bytes.len() == 32 {
                return Ok(key_bytes);
            }
        }
    }
    Err(CredentialError::NoEntry)
}

/// Write the encryption key for a user to config.
pub fn write_encryption_key(user_id: &str, key_bytes: &[u8]) -> Result<(), CredentialError> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let _ = user_id;
    let key_b64 = URL_SAFE_NO_PAD.encode(key_bytes);
    let mut cfg = load_config()?;
    cfg.encryption_key = key_b64;
    save_config_to_disk(&cfg)?;
    Ok(())
}

/// Compose `RestClient::retry_key_bundle` + `poll_key_bundle` so callers in
/// the CLI (initial `auth login`, `auth retry-key`, and the push/pull
/// auto-retry path) share a single implementation. Returns `true` if a
/// bundle arrived and the master key was persisted via
/// `auth_session::persist_received_master_key`; `false` otherwise.
///
/// Exercised end-to-end by the CLI integration tests against wiremock; not
/// re-tested in cinch-core because it is a pure composition over already
/// unit-tested functions.
pub async fn attempt_key_exchange_blocking(
    client: &crate::http::RestClient,
    priv_b64: &str,
    user_id: &str,
) -> bool {
    if client.retry_key_bundle().await.is_err() {
        return false;
    }
    poll_key_bundle(client, priv_b64, user_id).await
}

/// Poll `GET /auth/key-bundle` for up to 30s waiting for a key-bearer
/// device to publish our encrypted user-key bundle. Returns `true` if
/// a bundle arrived and the decrypted master key was persisted via
/// `credstore::write_encryption_key`; returns `false` on timeout or
/// any decode failure (with a single line printed to stderr per
/// observed failure mode, mirroring the original CLI behavior).
///
/// `priv_b64` is the local device's freshly-generated ephemeral
/// X25519 private key (matches the public key registered with the
/// relay during `auth login`); `user_id` scopes the credstore entry.
pub async fn poll_key_bundle(
    client: &crate::http::RestClient,
    priv_b64: &str,
    user_id: &str,
) -> bool {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        match client.get_key_bundle().await {
            Ok(bundle) if !bundle.encrypted_bundle.is_empty() => {
                let aes_key = match crate::crypto::derive_shared_key(
                    priv_b64,
                    &bundle.ephemeral_public_key,
                ) {
                    Ok(k) => k,
                    Err(e) => {
                        eprintln!("  ECDH derive failed: {}", e);
                        return false;
                    }
                };
                let user_key_bytes =
                    match crate::crypto::decrypt(&aes_key, &bundle.encrypted_bundle) {
                        Ok(b) => b,
                        Err(e) => {
                            eprintln!("  Bundle decrypt failed: {}", e);
                            return false;
                        }
                    };
                if user_key_bytes.len() != 32 {
                    eprintln!("  Unexpected user-key length: {}", user_key_bytes.len());
                    return false;
                }
                let mut key = [0u8; 32];
                key.copy_from_slice(&user_key_bytes);
                if let Err(e) = crate::auth_session::persist_received_master_key(user_id, &key) {
                    eprintln!("  Saving encryption key: {}", e);
                    return false;
                }
                return true;
            }
            // 404 means the desktop has not published yet — keep polling.
            _ => {}
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    false
}

/// rotate_credentials persists a new token after a WS `token_rotated` event.
pub fn rotate_credentials(
    user_id: &str,
    device_id: &str,
    token: &str,
    hostname: &str,
) -> Result<(), CredentialError> {
    let cfg = load_config()?;
    write_credentials(user_id, device_id, token, &cfg.relay_url, hostname)
}

/// stdout marker emitted by `cinch auth login --headless` so the
/// orchestrating side (e.g. `cinch device pair` running over SSH) can pick
/// up the device-code URL without parsing free-form output.
///
/// Format (single line, no trailing whitespace):
///   <<CINCH-DEVICE-CODE>>{"url":"...","user_code":"..."}<<END>>
pub const DEVICE_CODE_MARKER_START: &str = "<<CINCH-DEVICE-CODE>>";
pub const DEVICE_CODE_MARKER_END: &str = "<<END>>";

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DeviceCodeMarker {
    pub url: String,
    pub user_code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approve_command: Option<String>,
}

pub fn format_device_code_marker(url: &str, user_code: &str) -> String {
    let payload = serde_json::to_string(&DeviceCodeMarker {
        url: url.to_string(),
        user_code: user_code.to_string(),
        approve_command: Some(format!("cinch auth approve {}", user_code)),
    })
    .expect("serialize DeviceCodeMarker");
    format!(
        "{}{}{}",
        DEVICE_CODE_MARKER_START, payload, DEVICE_CODE_MARKER_END
    )
}

pub fn parse_device_code_marker(line: &str) -> Option<DeviceCodeMarker> {
    let start = line.find(DEVICE_CODE_MARKER_START)?;
    let after_start = start + DEVICE_CODE_MARKER_START.len();
    let end = line[after_start..].find(DEVICE_CODE_MARKER_END)?;
    let payload = &line[after_start..after_start + end];
    serde_json::from_str(payload).ok()
}

/// stdout marker emitted by the SSH pair script when the remote machine
/// has either reused an existing matching pairing or completed a fresh
/// device-code login. The orchestrating desktop uses this marker to
/// verify that the remote's `user_id` matches the local active profile —
/// without it, an exit-0 SSH session can falsely look successful when
/// the remote was already signed in as a different user (or `cinch auth
/// login` short-circuited before emitting any pairing evidence).
///
/// Format (single line, no trailing whitespace):
///   <<CINCH-PAIRED-OK>>{"user_id":"...","device_id":"...","reused":bool}<<END>>
pub const PAIRING_COMPLETE_MARKER_START: &str = "<<CINCH-PAIRED-OK>>";
pub const PAIRING_COMPLETE_MARKER_END: &str = "<<END>>";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PairingCompleteMarker {
    pub user_id: String,
    pub device_id: String,
    /// `true` when the remote already had a matching pairing on disk and
    /// the script skipped device-code; `false` when a fresh login ran.
    #[serde(default)]
    pub reused: bool,
}

pub fn parse_pairing_complete_marker(line: &str) -> Option<PairingCompleteMarker> {
    let start = line.find(PAIRING_COMPLETE_MARKER_START)?;
    let after_start = start + PAIRING_COMPLETE_MARKER_START.len();
    let end = line[after_start..].find(PAIRING_COMPLETE_MARKER_END)?;
    let payload = &line[after_start..after_start + end];
    serde_json::from_str(payload).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_key_format() {
        assert_eq!(account_key("u1", "d1"), "u1:d1");
    }
}

#[cfg(test)]
mod marker_tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = format_device_code_marker("https://x/y", "AB12");
        let parsed = parse_device_code_marker(&s).unwrap();
        assert_eq!(parsed.url, "https://x/y");
        assert_eq!(parsed.user_code, "AB12");
        assert_eq!(
            parsed.approve_command.as_deref(),
            Some("cinch auth approve AB12")
        );
    }

    #[test]
    fn old_marker_without_approve_command_still_parses() {
        let s = "<<CINCH-DEVICE-CODE>>{\"url\":\"https://x/y\",\"user_code\":\"AB12\"}<<END>>";
        let parsed = parse_device_code_marker(s).unwrap();
        assert_eq!(parsed.url, "https://x/y");
        assert_eq!(parsed.user_code, "AB12");
        assert_eq!(parsed.approve_command, None);
    }

    #[test]
    fn no_marker_returns_none() {
        assert!(parse_device_code_marker("just some log line").is_none());
    }

    #[test]
    fn truncated_marker_returns_none() {
        assert!(
            parse_device_code_marker("<<CINCH-DEVICE-CODE>>{\"url\":\"x\",\"user_code\":")
                .is_none()
        );
    }

    #[test]
    fn pairing_complete_marker_round_trip() {
        let s =
            "<<CINCH-PAIRED-OK>>{\"user_id\":\"u1\",\"device_id\":\"d1\",\"reused\":true}<<END>>";
        let parsed = parse_pairing_complete_marker(s).unwrap();
        assert_eq!(parsed.user_id, "u1");
        assert_eq!(parsed.device_id, "d1");
        assert!(parsed.reused);
    }

    #[test]
    fn pairing_complete_marker_defaults_reused_false() {
        let s = "<<CINCH-PAIRED-OK>>{\"user_id\":\"u1\",\"device_id\":\"d1\"}<<END>>";
        let parsed = parse_pairing_complete_marker(s).unwrap();
        assert!(!parsed.reused);
    }

    #[test]
    fn pairing_complete_marker_rejects_garbage() {
        assert!(parse_pairing_complete_marker("just a log line").is_none());
        assert!(parse_pairing_complete_marker("<<CINCH-PAIRED-OK>>not json<<END>>").is_none());
    }
}
