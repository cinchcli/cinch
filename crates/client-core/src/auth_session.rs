//! Single atomic entry point for installing a fresh sign-in onto disk.
//!
//! Replaces the historical pattern where each caller (CLI `run_login`,
//! desktop `sign_in`, desktop `handle_deeplink`) wrote credentials in three
//! independent steps:
//!
//!   1. `auth::write_credentials` — token + user_id + device_id + bump
//!   2. `credstore::write_encryption_key` — generated lazily, sometimes
//!      after the version bump fired
//!   3. `credstore::write_device_privkey` — generated lazily as well
//!
//! The lazy generation race meant the desktop FS watcher could fire on the
//! version bump from step 1 and adopt credentials before steps 2-3 had
//! produced the AES + X25519 material. `install_credentials` collapses all
//! three writes into a single transaction with exactly one
//! `credential_version` bump at the end.
//!
//! AES + X25519 are generated up-front (eager) and reused if the user
//! already has them on this machine.

use crate::auth::{load_multi_config, save_multi_config, CredentialError};
use crate::config::RelayProfile;
use crate::credstore;
use crate::crypto;

/// Inputs for an atomic credential install. Everything the relay returned
/// for a fresh device-code or pair handshake.
pub struct InstallParams<'a> {
    pub user_id: &'a str,
    pub device_id: &'a str,
    pub token: &'a str,
    pub relay_url: &'a str,
    pub hostname: &'a str,
    /// Optional pre-supplied X25519 device private key (base64url). When
    /// `None`, `install_credentials` generates a fresh keypair if the user
    /// does not already have one on this machine.
    pub device_private_key: Option<&'a str>,
    /// Verified email returned by the OAuth provider. Empty string when not available.
    pub email: &'a str,
    /// OAuth identity provider name ("google" or "github"). Empty string when not available.
    pub identity_provider: &'a str,
    /// Effective display name returned by the relay's poll response. Empty when
    /// no name is available; the CLI falls back to email/user_id in `auth status`.
    pub display_name: &'a str,
}

/// Outcome of `install_credentials` — useful for callers that want to
/// surface "this is the first sign-in on this machine" or report which
/// credstore backend was used.
#[derive(Debug, Clone)]
pub struct InstallOutcome {
    /// Active relay_id after the install (matches `MultiConfig.active_relay_id`).
    pub active_relay_id: String,
    /// New `credential_version` value persisted to disk.
    pub credential_version: u64,
    /// Backend used for the AES key write: `"keyring"` or `"plaintext"`.
    pub encryption_backend: &'static str,
    /// True when this call generated the AES key (vs. reused an existing one).
    /// After the E2EE-001 fix this is only true when the caller passed in a
    /// pre-existing master key path that wrote into the credstore; brand-new
    /// devices never fabricate a master key locally and instead surface
    /// `key_pending = true`.
    pub generated_encryption_key: bool,
    /// True when this call generated the X25519 device key.
    pub generated_device_private_key: bool,
    /// True when no user-scoped master AES key was available at install
    /// time. The device's X25519 keypair is registered with the relay, but
    /// every push/pull must trigger `retry_key_bundle` + `poll_key_bundle`
    /// until a paired device responds with an encrypted bundle.
    pub key_pending: bool,
}

/// Install credentials atomically: writes the AES user key + X25519 device
/// key first, then updates `~/.cinch/config.json` with token / user_id /
/// device_id / hostname / machine_id and bumps `credential_version` exactly
/// once at the end.
///
/// This is the only function CLI / desktop should call when persisting a
/// fresh sign-in. It guarantees the desktop FS watcher sees a fully-formed
/// credential set on the version bump.
pub fn install_credentials(p: InstallParams<'_>) -> Result<InstallOutcome, CredentialError> {
    if p.user_id.is_empty() || p.device_id.is_empty() || p.token.is_empty() {
        return Err(CredentialError::BadConfig(
            "user_id, device_id, token are required".into(),
        ));
    }

    // Step 1: never fabricate a local AES key. The user-scoped master key
    // is either already on disk (re-login on the same machine) or it must
    // arrive via ECDH from a paired device. Anything else silently
    // partitions this device from the rest of the account (E2EE-001).
    let generated_encryption_key = false;
    let encryption_backend: &'static str = "plaintext";
    let key_pending = credstore::read_encryption_key(p.user_id).is_none();

    // Step 2: ensure a per-device X25519 keypair exists.
    let mut generated_device_private_key = false;
    let device_priv_b64: String = if let Some(provided) = p.device_private_key {
        if !provided.is_empty() {
            credstore::write_device_privkey(p.user_id, p.device_id, provided)
                .map_err(|e| CredentialError::Io(format!("device key: {}", e)))?;
            provided.to_string()
        } else {
            install_or_reuse_device_privkey(
                p.user_id,
                p.device_id,
                &mut generated_device_private_key,
            )?
        }
    } else {
        install_or_reuse_device_privkey(p.user_id, p.device_id, &mut generated_device_private_key)?
    };

    // Step 3: update config.json — set/replace the active profile and bump
    // credential_version exactly once.
    let mut mc = load_multi_config()?;
    let next_version = mc
        .relays
        .iter()
        .map(|r| r.credential_version)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| CredentialError::BadConfig("credential_version overflow".into()))?;

    let machine_id = crate::machine::stable_machine_id();

    if let Some(profile) = mc.active_profile_mut() {
        profile.token = p.token.to_string();
        profile.user_id = p.user_id.to_string();
        profile.device_id = p.device_id.to_string();
        profile.relay_url = p.relay_url.to_string();
        profile.hostname = p.hostname.to_string();
        profile.device_private_key = device_priv_b64;
        profile.machine_id = machine_id;
        profile.credential_version = next_version;
        profile.key_pending = key_pending;
        if !p.email.is_empty() {
            profile.email = p.email.to_string();
        }
        if !p.identity_provider.is_empty() {
            profile.identity_provider = p.identity_provider.to_string();
        }
        if !p.display_name.is_empty() {
            profile.display_name = p.display_name.to_string();
        }
    } else {
        use ulid::Ulid;
        let label = url::Url::parse(p.relay_url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
            .unwrap_or_else(|| p.relay_url.to_string());
        let profile = RelayProfile {
            id: Ulid::new().to_string(),
            label,
            relay_url: p.relay_url.to_string(),
            user_id: p.user_id.to_string(),
            device_id: p.device_id.to_string(),
            hostname: p.hostname.to_string(),
            encryption_key: String::new(),
            device_private_key: device_priv_b64,
            credential_version: next_version,
            token: p.token.to_string(),
            machine_id,
            email: p.email.to_string(),
            identity_provider: p.identity_provider.to_string(),
            display_name: p.display_name.to_string(),
            key_pending,
        };
        let id = profile.id.clone();
        mc.relays.push(profile);
        mc.active_relay_id = Some(id);
    }

    let active_relay_id = mc.active_relay_id.clone().unwrap_or_default();
    save_multi_config(&mc)?;

    Ok(InstallOutcome {
        active_relay_id,
        credential_version: next_version,
        encryption_backend,
        generated_encryption_key,
        generated_device_private_key,
        key_pending,
    })
}

/// Error returned when the E2EE key is not available for a user.
#[derive(Debug, thiserror::Error)]
pub enum RequireKeyError {
    /// No master key on disk AND no record that one should arrive — the user
    /// genuinely has no credential for this user_id. CLI should prompt
    /// `cinch auth login`.
    #[error("encryption key not found for user")]
    Missing,
    /// The device registered its X25519 public key with the relay but the
    /// master AES key has not yet been received via ECDH. CLI should auto
    /// retry `attempt_key_exchange_blocking` once before failing.
    #[error("encryption key pending — waiting for paired device to share")]
    PendingExchange,
}

/// Persist a freshly-received 32-byte master AES key for `user_id` and
/// atomically clear `key_pending` on the active relay profile. Called by
/// `auth::poll_key_bundle` and by any other entry point (e.g. recovery-code
/// restore) that hands the device its real master key.
pub fn persist_received_master_key(
    user_id: &str,
    master_key: &[u8; 32],
) -> Result<(), CredentialError> {
    credstore::write_encryption_key(user_id, master_key)
        .map_err(|e| CredentialError::Io(format!("write encryption key: {}", e)))?;
    let mut mc = load_multi_config()?;
    // Receiving the master key is a credential state change the desktop FS
    // watcher must observe — it keys off `credential_version`. Always clear
    // `key_pending` AND bump the version (even if `key_pending` was already
    // false), otherwise a key that arrives after the flag was cleared lands
    // silently and the desktop never re-derives. Bump exactly once, monotonic
    // across all relay profiles (same scheme as `install_credentials`).
    let next_version = mc
        .relays
        .iter()
        .map(|r| r.credential_version)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| CredentialError::BadConfig("credential_version overflow".into()))?;
    if let Some(profile) = mc.active_profile_mut() {
        profile.key_pending = false;
        profile.credential_version = next_version;
        save_multi_config(&mc)?;
    }
    Ok(())
}

/// E2EE precondition. Returns the user's AES-256 key, `PendingExchange`
/// when the device is waiting on a paired peer to share the master key, or
/// `Missing` when no credential exists at all. Callers map:
/// - `Missing` → `ENCRYPTION_REQUIRED` exit code, prompt `cinch auth login`
/// - `PendingExchange` → `ENCRYPTION_PENDING` exit code, auto-retry key exchange
pub fn require_encryption_key(user_id: &str) -> Result<[u8; 32], RequireKeyError> {
    if user_id.is_empty() {
        return Err(RequireKeyError::Missing);
    }
    if let Some(key) = credstore::read_encryption_key(user_id) {
        return Ok(key);
    }
    // No key on disk — distinguish "pending ECDH" from "not signed in".
    if let Ok(cfg) = crate::auth::load_config() {
        if cfg.key_pending && cfg.user_id == user_id {
            return Err(RequireKeyError::PendingExchange);
        }
    }
    Err(RequireKeyError::Missing)
}

fn install_or_reuse_device_privkey(
    user_id: &str,
    device_id: &str,
    generated_flag: &mut bool,
) -> Result<String, CredentialError> {
    // Best-effort: if the active profile already has a non-empty device_private_key
    // for this same (user_id, device_id), reuse it. Otherwise generate one.
    let existing = load_multi_config()
        .ok()
        .and_then(|mc| mc.active_profile().cloned())
        .filter(|p| {
            p.user_id == user_id && p.device_id == device_id && !p.device_private_key.is_empty()
        })
        .map(|p| p.device_private_key);

    if let Some(priv_b64) = existing {
        return Ok(priv_b64);
    }

    let (priv_b64, _pub_b64) = crypto::generate_device_keypair();
    credstore::write_device_privkey(user_id, device_id, &priv_b64)
        .map_err(|e| CredentialError::Io(format!("device key: {}", e)))?;
    *generated_flag = true;
    Ok(priv_b64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests below mutate `HOME` to redirect `~/.cinch/config.json` into a
    /// per-test `tempdir`. Cargo runs tests in parallel by default, so
    /// without this lock two tests would see each other's HOME and read a
    /// half-cleaned config file. The lock is held only for the body of each
    /// HOME-mutating test, so non-HOME tests stay parallel.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn rejects_empty_required_fields() {
        let p = InstallParams {
            user_id: "",
            device_id: "d1",
            token: "tok",
            relay_url: "http://localhost:8080",
            hostname: "h",
            device_private_key: None,
            email: "",
            identity_provider: "",
            display_name: "",
        };
        assert!(install_credentials(p).is_err());
    }

    #[test]
    fn install_writes_display_name_into_profile() {
        let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", tmp.path());
        let outcome = install_credentials(InstallParams {
            user_id: "u1",
            device_id: "d1",
            token: "tok",
            relay_url: "https://r",
            hostname: "host",
            device_private_key: None,
            email: "alice@example.com",
            identity_provider: "github",
            display_name: "Alice Example",
        })
        .expect("install");
        assert!(!outcome.active_relay_id.is_empty());

        let cfg = crate::auth::load_config().expect("load");
        assert_eq!(cfg.display_name, "Alice Example");
    }

    #[test]
    fn require_encryption_key_errors_when_missing() {
        let err = require_encryption_key("test-no-key-x7k9q").unwrap_err();
        assert!(matches!(err, RequireKeyError::Missing));
    }

    #[test]
    fn require_encryption_key_errors_on_empty_user_id() {
        let err = require_encryption_key("").unwrap_err();
        assert!(matches!(err, RequireKeyError::Missing));
    }

    /// E2EE-001 phase A3: when the active relay profile is marked
    /// `key_pending`, `require_encryption_key` must surface a distinct
    /// `PendingExchange` error so the CLI can branch into auto-retry
    /// instead of asking the user to run `cinch auth login`.
    #[test]
    fn require_encryption_key_returns_pending_when_flagged() {
        let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", tmp.path());

        install_credentials(InstallParams {
            user_id: "u-pending",
            device_id: "d-pending",
            token: "tok",
            relay_url: "https://r",
            hostname: "host",
            device_private_key: None,
            email: "",
            identity_provider: "",
            display_name: "",
        })
        .expect("install");

        let err = require_encryption_key("u-pending").unwrap_err();
        assert!(
            matches!(err, RequireKeyError::PendingExchange),
            "expected PendingExchange, got {:?}",
            err
        );
    }

    /// E2EE-001: re-login on the same machine (master key already on disk
    /// from a prior key-exchange handshake) must REUSE the existing master
    /// key and clear `key_pending`. Otherwise an innocent re-login would
    /// wipe the device's working key and force another ECDH round trip.
    #[test]
    fn install_reuses_existing_master_key_and_leaves_pending_false() {
        let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", tmp.path());

        // Simulate the realistic post-handshake state: a first install ran
        // and left key_pending=true, then a paired device sent the bundle
        // and poll_key_bundle persisted the master key for this user.
        install_credentials(InstallParams {
            user_id: "u-reuse",
            device_id: "d-reuse",
            token: "tok-1",
            relay_url: "https://r",
            hostname: "host",
            device_private_key: None,
            email: "",
            identity_provider: "",
            display_name: "",
        })
        .expect("first install");
        let existing_key = [0x77u8; 32];
        credstore::write_encryption_key("u-reuse", &existing_key).expect("seed key");

        // Second install (re-login on the same machine) for the same user.
        let outcome = install_credentials(InstallParams {
            user_id: "u-reuse",
            device_id: "d-reuse",
            token: "tok-2",
            relay_url: "https://r",
            hostname: "host",
            device_private_key: None,
            email: "",
            identity_provider: "",
            display_name: "",
        })
        .expect("second install");

        assert!(
            !outcome.generated_encryption_key,
            "must not regenerate when the user already has a master key"
        );
        assert!(
            !outcome.key_pending,
            "key_pending must be false when the master key is already on disk"
        );
        let roundtrip = credstore::read_encryption_key("u-reuse").expect("read back");
        assert_eq!(
            roundtrip, existing_key,
            "install must not overwrite the existing master key"
        );
        let cfg = crate::auth::load_config().expect("load");
        assert!(
            !cfg.encryption_key.is_empty(),
            "config.encryption_key should reflect the preserved master key"
        );
        assert!(!cfg.key_pending);
    }

    /// E2EE-001: a brand-new device must NOT fabricate a fresh master AES key
    /// locally. Silently doing so partitions the device from every other
    /// device on the same account once `poll_key_bundle` times out — every
    /// push/pull then fails with `aead::Error`. Instead the device is marked
    /// `key_pending` and waits for a paired device to share the real master
    /// key via ECDH.
    /// E2EE-001 phase A2: when a key bundle arrives via the ECDH handshake,
    /// `persist_received_master_key` must (a) store the 32-byte master key
    /// for the user, and (b) atomically clear the `key_pending` flag on the
    /// active relay profile so subsequent push/pull no longer auto-retry.
    #[test]
    fn persist_received_master_key_clears_key_pending_and_stores_key() {
        let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", tmp.path());

        // First install leaves the device in key_pending state.
        install_credentials(InstallParams {
            user_id: "u-recv",
            device_id: "d-recv",
            token: "tok",
            relay_url: "https://r",
            hostname: "host",
            device_private_key: None,
            email: "",
            identity_provider: "",
            display_name: "",
        })
        .expect("install");
        let cfg_before = crate::auth::load_config().expect("load before");
        assert!(
            cfg_before.key_pending,
            "precondition: install left key_pending=true"
        );
        assert!(cfg_before.encryption_key.is_empty());

        // Simulate a successfully decrypted bundle landing.
        let master = [0xAAu8; 32];
        crate::auth_session::persist_received_master_key("u-recv", &master)
            .expect("persist master key");

        let cfg_after = crate::auth::load_config().expect("load after");
        assert!(
            !cfg_after.key_pending,
            "key_pending must be cleared on receipt"
        );
        let roundtrip = credstore::read_encryption_key("u-recv").expect("read key");
        assert_eq!(
            roundtrip, master,
            "master key must be persisted byte-for-byte"
        );
    }

    #[test]
    fn install_does_not_generate_local_aes_when_user_has_no_existing_key() {
        let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", tmp.path());
        let outcome = install_credentials(InstallParams {
            user_id: "u-new-001",
            device_id: "d-new-001",
            token: "tok",
            relay_url: "https://r",
            hostname: "host",
            device_private_key: None,
            email: "",
            identity_provider: "",
            display_name: "",
        })
        .expect("install");
        assert!(
            !outcome.generated_encryption_key,
            "install_credentials must not generate a fresh AES key for a brand-new device"
        );
        assert!(
            outcome.key_pending,
            "outcome.key_pending must be true when no master key was provided or found"
        );

        let cfg = crate::auth::load_config().expect("load");
        assert!(
            cfg.encryption_key.is_empty(),
            "encryption_key on disk must remain empty until a paired device shares it"
        );
        assert!(
            cfg.key_pending,
            "key_pending on disk must be true so push/pull know to auto-retry"
        );
    }
}
