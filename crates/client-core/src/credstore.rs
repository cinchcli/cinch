//! Credential store abstraction — plaintext config.json as canonical store.
//!
//! `~/.cinch/config.json` (mode 0600) is the single credential store on every
//! platform. Service name and account formats remain lock-step with the Go CLI's
//! `cinch/cmd/internal/credstore/store.go` for the Keychain migration window:
//!
//!   service = "com.cinchcli"
//!   account = "<user_id>:<device_id>"             // auth token
//!   account = "encryption:<user_id>"              // 32-byte AES key (base64url)
//!   account = "device-privkey:<user_id>:<device_id>"  // X25519 private key (base64url)
//!
//! **Migration.** CLI builds prior to 2026-05-08 wrote credentials to the OS
//! Keychain (service `com.cinchcli`, legacy `com.cinch.app`). The `read_*`
//! helpers transparently fall back to both Keychain services on a plaintext
//! miss and copy the value forward to config.json on first success. No writes
//! go to the Keychain any longer.

use crate::auth::{LEGACY_SERVICE_NAME, SERVICE_NAME};

pub fn account_key(user_id: &str, device_id: &str) -> String {
    format!("{}:{}", user_id, device_id)
}

pub fn encryption_account_key(user_id: &str) -> String {
    format!("encryption:{}", user_id)
}

pub fn device_privkey_account_key(user_id: &str, device_id: &str) -> String {
    format!("device-privkey:{}:{}", user_id, device_id)
}

#[derive(Debug, thiserror::Error)]
pub enum CredstoreError {
    #[error("no entry")]
    NoEntry,
    #[error("backend: {0}")]
    Backend(String),
}

pub trait Credstore: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, CredstoreError>;
    fn set(&self, account: &str, value: &str) -> Result<(), CredstoreError>;
    fn delete(&self, account: &str) -> Result<(), CredstoreError>;
    fn backend_name(&self) -> &'static str;
}

/// macOS Keychain / Linux Secret Service / Windows Credential Manager backend
/// pinned to a specific service name. Used only for one-time migration reads
/// from pre-2026-05-08 CLI builds. No new writes go here.
pub struct KeyringStore {
    service: &'static str,
}

impl KeyringStore {
    pub fn canonical() -> Self {
        Self {
            service: SERVICE_NAME,
        }
    }

    pub fn legacy() -> Self {
        Self {
            service: LEGACY_SERVICE_NAME,
        }
    }
}

/// Prefix that `zalando/go-keyring` adds to values it writes on macOS so
/// arbitrary bytes survive the Keychain string interface. The Rust
/// `keyring` crate does not use this wrapper, so we transparently
/// unwrap on read to stay byte-compatible with any Go CLI entries still
/// sitting in the Keychain during the migration window.
const GO_KEYRING_BASE64_PREFIX: &str = "go-keyring-base64:";

fn unwrap_go_keyring(value: String) -> String {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    if let Some(rest) = value.strip_prefix(GO_KEYRING_BASE64_PREFIX) {
        if let Ok(bytes) = STANDARD.decode(rest) {
            if let Ok(s) = String::from_utf8(bytes) {
                return s;
            }
        }
    }
    value
}

impl Credstore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<String>, CredstoreError> {
        let entry = keyring::Entry::new(self.service, account)
            .map_err(|e| CredstoreError::Backend(e.to_string()))?;
        match entry.get_password() {
            Ok(v) => Ok(Some(unwrap_go_keyring(v))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(CredstoreError::Backend(e.to_string())),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), CredstoreError> {
        let entry = keyring::Entry::new(self.service, account)
            .map_err(|e| CredstoreError::Backend(e.to_string()))?;
        entry
            .set_password(value)
            .map_err(|e| CredstoreError::Backend(e.to_string()))
    }

    fn delete(&self, account: &str) -> Result<(), CredstoreError> {
        let entry = keyring::Entry::new(self.service, account)
            .map_err(|e| CredstoreError::Backend(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(CredstoreError::Backend(e.to_string())),
        }
    }

    fn backend_name(&self) -> &'static str {
        if self.service == LEGACY_SERVICE_NAME {
            "keyring-legacy"
        } else {
            "keyring"
        }
    }
}

/// Plaintext store — reads/writes via `client_core::auth` config helpers.
/// Only token, encryption_key, device_private_key are persisted; other
/// account names return None.
pub struct PlaintextStore;

impl Credstore for PlaintextStore {
    fn get(&self, account: &str) -> Result<Option<String>, CredstoreError> {
        let cfg = crate::auth::load_config().map_err(|e| CredstoreError::Backend(e.to_string()))?;
        if account.starts_with("encryption:") {
            let expected = encryption_account_key(&cfg.user_id);
            if account == expected {
                return Ok(non_empty(cfg.encryption_key));
            }
            return Ok(None);
        }
        if account.starts_with("device-privkey:") {
            let expected = device_privkey_account_key(&cfg.user_id, &cfg.active_device_id);
            if account == expected {
                return Ok(non_empty(cfg.device_private_key));
            }
            return Ok(None);
        }
        let expected = account_key(&cfg.user_id, &cfg.active_device_id);
        if account == expected {
            return Ok(non_empty(cfg.token));
        }
        Ok(None)
    }

    fn set(&self, _account: &str, _value: &str) -> Result<(), CredstoreError> {
        Err(CredstoreError::Backend(
            "plaintext credstore writes go through client_core::auth helpers".into(),
        ))
    }

    fn delete(&self, _account: &str) -> Result<(), CredstoreError> {
        Err(CredstoreError::Backend(
            "plaintext credstore deletes go through client_core::auth helpers".into(),
        ))
    }

    fn backend_name(&self) -> &'static str {
        "plaintext"
    }
}

fn non_empty(s: String) -> Option<String> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

/// Returns the canonical credential store. Always plaintext (config.json,
/// mode 0600). Keychain entries from prior CLI builds are read by the
/// `read_*` migration helpers below, never by this function.
pub fn detect() -> Box<dyn Credstore> {
    Box::new(PlaintextStore)
}

/// Read `account` from `plaintext`; on miss, try each `fallback` store in
/// order. On a fallback hit, call `plaintext_writer` to copy the value
/// forward. Returns the value or `None` if all stores miss.
fn get_with_migration_via(
    plaintext: &dyn Credstore,
    fallbacks: &[&dyn Credstore],
    plaintext_writer: impl FnOnce(&str) -> Result<(), CredstoreError>,
    account: &str,
) -> Option<String> {
    if let Ok(Some(value)) = plaintext.get(account) {
        return Some(value);
    }
    for fb in fallbacks {
        if let Ok(Some(value)) = fb.get(account) {
            // Best-effort copy-forward. Failure is non-fatal — subsequent
            // reads hit Keychain again until plaintext succeeds.
            let _ = plaintext_writer(&value);
            return Some(value);
        }
    }
    None
}

/// Read `account` from plaintext first, falling back to Keychain (canonical
/// then legacy services) for users upgrading from the Keychain-era CLI.
/// On a Keychain hit the value is copied forward to config.json.
fn get_with_keyring_migration(
    plaintext_writer: impl FnOnce(&str) -> Result<(), CredstoreError>,
    account: &str,
) -> Option<String> {
    let canonical = KeyringStore::canonical();
    let legacy = KeyringStore::legacy();
    get_with_migration_via(
        &PlaintextStore,
        &[&canonical as &dyn Credstore, &legacy as &dyn Credstore],
        plaintext_writer,
        account,
    )
}

/// Read the encryption key for `user_id`. Returns the 32-byte AES key or `None`.
pub fn read_encryption_key(user_id: &str) -> Option<[u8; 32]> {
    if user_id.is_empty() {
        return None;
    }
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let acct = encryption_account_key(user_id);
    let user_id_owned = user_id.to_string();
    let copy_forward = move |value: &str| -> Result<(), CredstoreError> {
        let bytes = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|e| CredstoreError::Backend(e.to_string()))?;
        if bytes.len() != 32 {
            return Err(CredstoreError::Backend("not 32 bytes".into()));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        crate::auth::write_encryption_key(&user_id_owned, &key)
            .map_err(|e| CredstoreError::Backend(e.to_string()))
    };
    let b64 = get_with_keyring_migration(copy_forward, &acct)?;
    let bytes = URL_SAFE_NO_PAD.decode(&b64).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Some(key)
}

/// Persist a 32-byte AES encryption key for `user_id` to config.json.
/// Always returns `"plaintext"`.
pub fn write_encryption_key(user_id: &str, key: &[u8; 32]) -> Result<&'static str, CredstoreError> {
    crate::auth::write_encryption_key(user_id, key)
        .map_err(|e| CredstoreError::Backend(e.to_string()))?;
    Ok("plaintext")
}

/// Read the base64url-encoded X25519 private key for `(user_id, device_id)`.
/// Returns `None` when the key has not yet been written for this pair.
pub fn read_device_privkey(user_id: &str, device_id: &str) -> Option<String> {
    if user_id.is_empty() || device_id.is_empty() {
        return None;
    }
    let acct = device_privkey_account_key(user_id, device_id);
    let copy_forward = |value: &str| -> Result<(), CredstoreError> {
        let mut cfg =
            crate::auth::load_config().map_err(|e| CredstoreError::Backend(e.to_string()))?;
        cfg.device_private_key = value.to_string();
        crate::auth::save_config_to_disk(&cfg).map_err(|e| CredstoreError::Backend(e.to_string()))
    };
    let value = get_with_keyring_migration(copy_forward, &acct)?;
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Persist a base64url-encoded X25519 private key for `(user_id, device_id)`
/// to config.json. Always returns `"plaintext"`.
pub fn write_device_privkey(
    user_id: &str,
    device_id: &str,
    privkey_b64: &str,
) -> Result<&'static str, CredstoreError> {
    let _ = (user_id, device_id);
    let mut cfg = crate::auth::load_config().map_err(|e| CredstoreError::Backend(e.to_string()))?;
    cfg.device_private_key = privkey_b64.to_string();
    crate::auth::save_config_to_disk(&cfg).map_err(|e| CredstoreError::Backend(e.to_string()))?;
    Ok("plaintext")
}

/// Read the auth token for the active (user, device) pair.
pub fn read_token(user_id: &str, device_id: &str) -> Option<String> {
    if user_id.is_empty() || device_id.is_empty() {
        return None;
    }
    let acct = account_key(user_id, device_id);
    // Token write-forward writes only cfg.token — we don't want to bump
    // credential_version during a passive read.
    let copy_forward = |value: &str| -> Result<(), CredstoreError> {
        let mut cfg =
            crate::auth::load_config().map_err(|e| CredstoreError::Backend(e.to_string()))?;
        cfg.token = value.to_string();
        crate::auth::save_config_to_disk(&cfg).map_err(|e| CredstoreError::Backend(e.to_string()))
    };
    get_with_keyring_migration(copy_forward, &acct).filter(|t| !t.is_empty())
}

/// Best-effort delete of all Keychain entries this user/device might have
/// from prior Keychain-era CLI builds. Errors are swallowed: the goal is
/// hygiene, not correctness — config.json is the source of truth.
pub fn wipe_keyring_for(user_id: &str, device_id: &str) {
    if user_id.is_empty() {
        return;
    }
    let mut accounts = vec![encryption_account_key(user_id)];
    if !device_id.is_empty() {
        accounts.push(account_key(user_id, device_id));
        accounts.push(device_privkey_account_key(user_id, device_id));
    }
    for service in [KeyringStore::canonical(), KeyringStore::legacy()] {
        for acct in &accounts {
            let _ = service.delete(acct);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_keys_match_go_format() {
        assert_eq!(account_key("u1", "d1"), "u1:d1");
        assert_eq!(encryption_account_key("u1"), "encryption:u1");
        assert_eq!(
            device_privkey_account_key("u1", "d1"),
            "device-privkey:u1:d1"
        );
    }

    #[test]
    fn go_keyring_unwrap_roundtrips() {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;
        let raw = "abcXYZ_-=";
        let wrapped = format!("{}{}", GO_KEYRING_BASE64_PREFIX, STANDARD.encode(raw));
        assert!(wrapped.starts_with(GO_KEYRING_BASE64_PREFIX));
        assert_eq!(unwrap_go_keyring(wrapped), raw);
    }

    #[test]
    fn go_keyring_unwrap_passthrough_for_unwrapped_values() {
        let raw = "plain-string-without-prefix".to_string();
        assert_eq!(unwrap_go_keyring(raw.clone()), raw);
    }

    // --- migration shape tests (in-memory fakes; no real Keychain access) ---

    struct InMemoryStore {
        map: std::sync::Mutex<std::collections::HashMap<String, String>>,
        name: &'static str,
    }
    impl InMemoryStore {
        fn new(name: &'static str) -> Self {
            Self {
                map: Default::default(),
                name,
            }
        }
        fn seed(&self, k: &str, v: &str) {
            self.map.lock().unwrap().insert(k.into(), v.into());
        }
    }
    impl Credstore for InMemoryStore {
        fn get(&self, account: &str) -> Result<Option<String>, CredstoreError> {
            Ok(self.map.lock().unwrap().get(account).cloned())
        }
        fn set(&self, account: &str, value: &str) -> Result<(), CredstoreError> {
            self.map
                .lock()
                .unwrap()
                .insert(account.into(), value.into());
            Ok(())
        }
        fn delete(&self, account: &str) -> Result<(), CredstoreError> {
            self.map.lock().unwrap().remove(account);
            Ok(())
        }
        fn backend_name(&self) -> &'static str {
            self.name
        }
    }

    #[test]
    fn migration_reads_from_canonical_and_copies_forward() {
        let plaintext = InMemoryStore::new("plaintext");
        let canonical = InMemoryStore::new("canonical");
        canonical.seed("encryption:u1", "AAAA");

        let mut copied = None;
        let writer = |v: &str| -> Result<(), CredstoreError> {
            copied = Some(v.to_string());
            Ok(())
        };
        let v = get_with_migration_via(
            &plaintext,
            &[&canonical as &dyn Credstore],
            writer,
            "encryption:u1",
        );
        assert_eq!(v.as_deref(), Some("AAAA"));
        assert_eq!(
            copied.as_deref(),
            Some("AAAA"),
            "must copy forward to plaintext"
        );
    }

    #[test]
    fn migration_falls_through_canonical_to_legacy() {
        let plaintext = InMemoryStore::new("plaintext");
        let canonical = InMemoryStore::new("canonical");
        let legacy = InMemoryStore::new("legacy");
        legacy.seed("encryption:u1", "BBBB");

        let writer = |_: &str| -> Result<(), CredstoreError> { Ok(()) };
        let v = get_with_migration_via(
            &plaintext,
            &[&canonical as &dyn Credstore, &legacy as &dyn Credstore],
            writer,
            "encryption:u1",
        );
        assert_eq!(v.as_deref(), Some("BBBB"));
    }

    #[test]
    fn migration_skips_writer_when_plaintext_already_has_value() {
        let plaintext = InMemoryStore::new("plaintext");
        plaintext.seed("encryption:u1", "EXISTING");
        let canonical = InMemoryStore::new("canonical");
        canonical.seed("encryption:u1", "STALE");

        let mut writer_called = false;
        let writer = |_: &str| -> Result<(), CredstoreError> {
            writer_called = true;
            Ok(())
        };
        let v = get_with_migration_via(
            &plaintext,
            &[&canonical as &dyn Credstore],
            writer,
            "encryption:u1",
        );
        assert_eq!(v.as_deref(), Some("EXISTING"));
        assert!(
            !writer_called,
            "must not overwrite plaintext when it already has the value"
        );
    }
}
