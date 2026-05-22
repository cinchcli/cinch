//! Pure URL / deep-link validation helpers. Extracted from `lib.rs` so
//! they can be unit-tested without a running Tauri app.

/// Validate that a relay URL uses http(s) and has a non-empty host.
/// Prevents deep-link injection where relay_url points to attacker infrastructure.
pub(crate) fn validate_relay_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|_| format!("invalid relay URL: {}", url))?;
    match parsed.scheme() {
        "https" | "http" => {}
        s => return Err(format!("relay URL scheme must be http(s), got: {}", s)),
    }
    if parsed.host().is_none() {
        return Err("relay URL must have a host".into());
    }
    Ok(())
}

/// Validate an incoming `cinch://auth/callback` deep-link against the relay URL
/// that was recorded when the user actually initiated a login.
///
/// Returns `Ok(())` only when:
/// - `pending_relay_url` is `Some` (a login was actively in progress), AND
/// - it matches `callback_relay_url` exactly (prevents relay-substitution attacks).
///
/// This is a pure function so it can be unit-tested without a running Tauri app.
pub(crate) fn validate_auth_callback(
    pending_relay_url: Option<&str>,
    callback_relay_url: &str,
) -> Result<(), &'static str> {
    match pending_relay_url {
        None => Err("no pending auth — deep-link rejected (no login was initiated)"),
        Some(pending) => {
            let pending_norm = pending.trim_end_matches('/');
            let callback_norm = callback_relay_url.trim_end_matches('/');
            if pending_norm != callback_norm {
                Err("relay_url mismatch — deep-link rejected (possible relay-substitution attack)")
            } else {
                Ok(())
            }
        }
    }
}
