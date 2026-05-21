//! Integration tests against a real relay instance (Docker Compose).
//!
//! All tests are skipped when RELAY_INTEGRATION_URL is not set.
//! Run via: cinch/scripts/integration/relay-auth.sh

use client_core::http::{HttpError, RestClient};
use client_core::version::ClientInfo;

fn relay_url() -> Option<String> {
    std::env::var("RELAY_INTEGRATION_URL").ok()
}

/// Login via the legacy /auth/login endpoint (available when OAuth is not
/// configured). Returns (token, user_id, device_id).
///
/// X-Forwarded-For is set to 127.0.0.1 so the relay treats the request as
/// loopback and skips its per-IP rate limit — the relay's own code exempts
/// 127.0.0.1/::1 for "smoke tests, local dev" (handler.go:238-241).
async fn login(relay: &str) -> (String, String, String) {
    let body: serde_json::Value = reqwest::Client::new()
        .post(format!("{}/auth/login", relay))
        .header("X-Forwarded-For", "127.0.0.1")
        .json(&serde_json::json!({"hostname": "integration-test-host"}))
        .send()
        .await
        .expect("POST /auth/login failed")
        .json()
        .await
        .expect("POST /auth/login: non-JSON body");
    let token = body["token"]
        .as_str()
        .expect("token field missing")
        .to_string();
    let user_id = body["user_id"]
        .as_str()
        .expect("user_id field missing")
        .to_string();
    let device_id = body["device_id"]
        .as_str()
        .expect("device_id field missing")
        .to_string();
    (token, user_id, device_id)
}

// ──────────────────────────────────────────────────────────────
// retry_key_bundle — fix: 401 now surfaces as HttpError::Unauthorized
// (previously it surfaced as HttpError::Relay { status: 401 } which
// gave the user a confusing "Retry failed: relay error (401)" message
// instead of prompting them to re-authenticate)
// ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn retry_key_bundle_with_invalid_token_returns_unauthorized() {
    let Some(relay) = relay_url() else {
        return;
    };

    let client = RestClient::new(&relay, "bogus-token-xyz", ClientInfo::for_test()).unwrap();
    let err = client
        .retry_key_bundle()
        .await
        .expect_err("expected error for invalid token");

    assert!(
        matches!(err, HttpError::Unauthorized),
        "expected HttpError::Unauthorized, got: {err:?}",
    );
}

#[tokio::test]
async fn retry_key_bundle_without_registered_pubkey_returns_bad_request() {
    let Some(relay) = relay_url() else {
        return;
    };

    // Login creates a device but does NOT register a public key.
    // The relay must reply 400 "device has not registered a public key yet".
    let (token, _, _) = login(&relay).await;
    let client = RestClient::new(&relay, &token, ClientInfo::for_test()).unwrap();
    let err = client
        .retry_key_bundle()
        .await
        .expect_err("expected 400 for device without pubkey");

    assert!(
        matches!(err, HttpError::Relay { status: 400, .. }),
        "expected HttpError::Relay {{ status: 400 }}, got: {err:?}",
    );
}

// ──────────────────────────────────────────────────────────────
// list_devices — used by run_status server-validation (fix: auth
// status now hits the relay instead of trusting local config)
// ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn list_devices_with_invalid_token_returns_unauthorized() {
    let Some(relay) = relay_url() else {
        return;
    };

    let client = RestClient::new(&relay, "bogus-token-xyz", ClientInfo::for_test()).unwrap();
    let err = client
        .list_devices()
        .await
        .expect_err("expected error for invalid token");

    assert!(
        matches!(err, HttpError::Unauthorized),
        "expected HttpError::Unauthorized, got: {err:?}",
    );
}

#[tokio::test]
async fn list_devices_with_valid_token_returns_current_device() {
    let Some(relay) = relay_url() else {
        return;
    };

    let (token, _, device_id) = login(&relay).await;
    let client = RestClient::new(&relay, &token, ClientInfo::for_test()).unwrap();
    let devices = client
        .list_devices()
        .await
        .expect("list_devices should succeed with a valid token");

    assert!(
        !devices.is_empty(),
        "expected at least the current device after login"
    );
    assert!(
        devices.iter().any(|d| d.id == device_id),
        "current device_id {device_id} not found in device list: {devices:?}",
    );
}
