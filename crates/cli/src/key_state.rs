//! CLI-side helpers for translating `client_core::auth_session::RequireKeyError`
//! into structured CLI exit errors and human-readable status lines.
//!
//! Centralizing this here keeps `cinch push`, `cinch pull`, and
//! `cinch auth status` consistent: a `PendingExchange` key state must always
//! map to the `ENCRYPTION_PENDING` exit code and a remediation hint that
//! mentions `cinch auth retry-key`, never `cinch auth login`.

use client_core::auth_session::RequireKeyError;
use client_core::config::Config;
use client_core::http::RestClient;

use crate::exit::{ExitError, ENCRYPTION_PENDING, ENCRYPTION_REQUIRED};

/// Map a `RequireKeyError` to a structured `ExitError` with the right exit
/// code and remediation hint. Callers should prefer this over building the
/// `ExitError` ad-hoc so the wording stays consistent across commands.
pub fn classify_key_error(err: RequireKeyError) -> ExitError {
    match err {
        RequireKeyError::Missing => ExitError::new(
            ENCRYPTION_REQUIRED,
            "Encryption key missing. End-to-end encryption is required.",
            "Run: cinch auth login",
        ),
        RequireKeyError::PendingExchange => ExitError::new(
            ENCRYPTION_PENDING,
            "Encryption key pending. Waiting for a paired device to share it.",
            "Open the Cinch desktop app on a paired device, then: cinch auth retry-key",
        ),
    }
}

/// Ensure the user has a master AES key on disk before the caller does any
/// encrypt/decrypt work. Returns:
/// * `Ok(())` — key present (now); caller can proceed.
/// * `Err(ENCRYPTION_PENDING)` — key still not available after a one-shot
///   `attempt_key_exchange_blocking` round.
/// * `Err(ENCRYPTION_REQUIRED)` — no credential at all (user must
///   `cinch auth login`).
///
/// Designed to be called at most once per CLI invocation, before push/pull
/// reach for the key. Stays silent when the key is already present so the
/// happy path adds zero latency.
///
/// Integration-test plumbing (wiremock against /auth/key-bundle/retry and
/// /auth/key-bundle) is deferred to a follow-up; today this is exercised
/// end-to-end via manual smoke runs against the hosted relay.
pub async fn ensure_master_key(cfg: &Config, client: &RestClient) -> Result<(), ExitError> {
    use client_core::auth_session::require_encryption_key;
    match require_encryption_key(&cfg.user_id) {
        Ok(_) => return Ok(()),
        Err(RequireKeyError::Missing) => return Err(classify_key_error(RequireKeyError::Missing)),
        Err(RequireKeyError::PendingExchange) => {
            // fall through to the retry attempt below
        }
    }
    let priv_b64 =
        match client_core::credstore::read_device_privkey(&cfg.user_id, &cfg.active_device_id) {
            Some(s) => s,
            None => return Err(classify_key_error(RequireKeyError::Missing)),
        };
    eprintln!(
        "\u{23F3} Encryption key not yet received. Asking paired devices to share it (up to 30s)..."
    );
    let ok =
        client_core::auth::attempt_key_exchange_blocking(client, &priv_b64, &cfg.user_id).await;
    if !ok {
        return Err(classify_key_error(RequireKeyError::PendingExchange));
    }
    // Re-check post-handshake: should be Ok now, but handle the unlikely
    // race where the bundle landed and was immediately overwritten.
    require_encryption_key(&cfg.user_id)
        .map(|_| ())
        .map_err(classify_key_error)
}

/// Render the `auth status` "Key:" line given the active config and whatever
/// the credstore returned. Pure function so it can be unit-tested without
/// touching disk. Returns `(line, optional hint)` where each is a single
/// line that the caller `eprintln!`s with the existing two-space indent.
pub fn describe_key_state(cfg: &Config, key_in_credstore: bool) -> (String, Option<String>) {
    if key_in_credstore {
        return ("Key:   \u{2713} present".to_string(), None);
    }
    if cfg.key_pending {
        return (
            "Key:   \u{23F3} pending (waiting for a paired device to share the encryption key)"
                .to_string(),
            Some("Try: cinch auth retry-key".to_string()),
        );
    }
    (
        "Key:   \u{26A0} missing".to_string(),
        Some("Try: cinch auth retry-key".to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(key_pending: bool, user_id: &str) -> Config {
        Config {
            user_id: user_id.to_string(),
            key_pending,
            ..Config::default()
        }
    }

    #[test]
    fn classify_pending_uses_pending_exit_code_and_retry_key_hint() {
        let err = classify_key_error(RequireKeyError::PendingExchange);
        assert_eq!(err.code, ENCRYPTION_PENDING);
        assert!(
            err.message.to_lowercase().contains("pending"),
            "message must mention pending state, got {:?}",
            err.message
        );
        assert!(
            err.fix.contains("cinch auth retry-key"),
            "fix must point users to retry-key, got {:?}",
            err.fix
        );
    }

    #[test]
    fn classify_missing_uses_encryption_required_and_login_hint() {
        let err = classify_key_error(RequireKeyError::Missing);
        assert_eq!(err.code, ENCRYPTION_REQUIRED);
        assert!(err.fix.contains("cinch auth login"));
    }

    #[test]
    fn describe_present_returns_check_mark_no_hint() {
        let (line, hint) = describe_key_state(&cfg_with(false, "u"), true);
        assert!(line.contains("present"));
        assert!(hint.is_none());
    }

    #[test]
    fn describe_pending_returns_hourglass_and_retry_hint() {
        let (line, hint) = describe_key_state(&cfg_with(true, "u"), false);
        assert!(line.contains("pending"));
        assert!(hint.unwrap().contains("cinch auth retry-key"));
    }

    #[test]
    fn describe_missing_returns_warning_and_retry_hint() {
        let (line, hint) = describe_key_state(&cfg_with(false, "u"), false);
        assert!(line.contains("missing"));
        assert!(hint.is_some());
    }
}
