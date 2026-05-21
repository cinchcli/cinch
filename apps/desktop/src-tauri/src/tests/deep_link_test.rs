#[cfg(test)]
mod tests {
    use crate::validate_auth_callback;

    // Finding 1 — no pending auth state must be rejected outright.
    #[test]
    fn test_auth_callback_requires_pending_state() {
        let result = validate_auth_callback(None, "https://relay.example.com");
        assert!(
            result.is_err(),
            "expected Err when no login was initiated, got Ok"
        );
    }

    // Finding 1 — callback relay_url differs from the one we opened the browser for.
    #[test]
    fn test_auth_callback_rejects_url_mismatch() {
        let result = validate_auth_callback(
            Some("https://relay.example.com"),
            "https://attacker.example.com",
        );
        assert!(
            result.is_err(),
            "expected Err on relay_url mismatch, got Ok"
        );
    }

    // Happy path — pending URL matches callback URL exactly.
    #[test]
    fn test_auth_callback_accepts_matching_url() {
        let result = validate_auth_callback(
            Some("https://relay.example.com"),
            "https://relay.example.com",
        );
        assert!(
            result.is_ok(),
            "expected Ok when pending relay matches callback, got Err: {:?}",
            result.err()
        );
    }

    // Edge: empty pending string should not accidentally match a real URL.
    #[test]
    fn test_auth_callback_rejects_empty_pending_vs_real_url() {
        let result = validate_auth_callback(Some(""), "https://relay.example.com");
        assert!(
            result.is_err(),
            "expected Err when pending is empty string, got Ok"
        );
    }

    // Edge: both empty strings match — degenerate but consistent.
    #[test]
    fn test_auth_callback_accepts_matching_empty_strings() {
        let result = validate_auth_callback(Some(""), "");
        assert!(
            result.is_ok(),
            "expected Ok when both pending and callback are empty, got Err"
        );
    }

    // M2: trailing-slash normalization — relay stores trimmed URL but callback
    // may carry a trailing slash; both directions must match.
    #[test]
    fn test_auth_callback_accepts_trailing_slash_on_callback() {
        let result = validate_auth_callback(
            Some("https://relay.example.com"),
            "https://relay.example.com/",
        );
        assert!(
            result.is_ok(),
            "expected Ok when callback has trailing slash vs stored URL without, got Err: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_auth_callback_accepts_trailing_slash_on_pending() {
        let result = validate_auth_callback(
            Some("https://relay.example.com/"),
            "https://relay.example.com",
        );
        assert!(
            result.is_ok(),
            "expected Ok when pending has trailing slash vs callback without, got Err: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_auth_callback_rejects_mismatch_with_trailing_slash() {
        let result = validate_auth_callback(
            Some("https://relay.example.com/"),
            "https://attacker.example.com/",
        );
        assert!(
            result.is_err(),
            "expected Err on relay_url mismatch even with trailing slashes, got Ok"
        );
    }
}
