//! HTTP-level integration tests for the device-code login flow.
//!
//! The existing inline tests in `http.rs` only check that
//! `DeviceCodeStartRequest` serializes to the right JSON shape. This file
//! exercises the actual `RestClient::start_device_code` and
//! `::poll_device_code` async paths against a `wiremock::MockServer`,
//! which is what the `cinch auth login` polling loop in
//! `crates/cli/src/commands/auth.rs` drives over the network. The status
//! string vocabulary (`pending` / `complete` / `expired` / `denied`) is a
//! relay ↔ client contract — the poller in `auth.rs` matches against
//! those exact strings — so each branch gets its own round-trip test.
//!
//! Verified manually against a real relay at `https://api.cinchcli.com`
//! during the 0.3.2 release rollout (atlas_1 ↔ atlas_0 cross-device
//! approval, May 21 2026).

use client_core::http::RestClient;
use client_core::version::ClientInfo;
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn client(server: &MockServer) -> RestClient {
    // start_device_code is unauthenticated — the device-code flow is
    // exactly how the relay learns about a new device — so the token
    // string is irrelevant here. poll_device_code also doesn't require
    // auth (the device_code itself is the capability).
    RestClient::new(server.uri(), "", ClientInfo::for_test()).unwrap()
}

#[tokio::test]
async fn start_device_code_round_trip() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/auth/device-code"))
        .and(body_json(serde_json::json!({
            "hostname": "dev-box-3",
            "machine_id": "m1",
            "user_hint": "alice@example.com",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "device_code": "dev-XYZ",
            "user_code": "ABCD-EFGH",
            "verification_uri": "https://relay.example/auth/browser?device_code=ABCD-EFGH",
            "expires_in": 600,
            "interval": 5,
            "interval_ms": 750,
        })))
        .expect(1)
        .mount(&server)
        .await;

    let resp = client(&server)
        .start_device_code(&server.uri(), "dev-box-3", "m1", Some("alice@example.com"))
        .await
        .expect("start_device_code succeeds");

    assert_eq!(resp.device_code, "dev-XYZ");
    assert_eq!(resp.user_code, "ABCD-EFGH");
    assert_eq!(
        resp.verification_uri,
        "https://relay.example/auth/browser?device_code=ABCD-EFGH"
    );
    assert_eq!(resp.interval, 5);
    assert_eq!(resp.interval_ms, Some(750));
}

#[tokio::test]
async fn start_device_code_omits_machine_id_when_empty_on_wire() {
    // Matching the `--machine-id ""` path in `run_login`: an empty string
    // must serialize to a missing field, not `"machine_id": ""`. Relay
    // dedup keys on the field's presence.
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/auth/device-code"))
        .and(body_json(serde_json::json!({
            "hostname": "dev-box-3",
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "device_code": "dev-XYZ",
            "user_code": "ABCD-EFGH",
            "verification_uri": "https://relay.example/auth/browser?device_code=ABCD-EFGH",
            "expires_in": 600,
            "interval": 5,
        })))
        .expect(1)
        .mount(&server)
        .await;

    client(&server)
        .start_device_code(&server.uri(), "dev-box-3", "", None)
        .await
        .expect("start_device_code succeeds without machine_id / user_hint");
}

#[tokio::test]
async fn poll_device_code_pending_then_complete() {
    // Simulates the two-state happy path that drives `await_approval` in
    // commands/auth.rs:511. First poll returns "pending" (any non-terminal
    // status falls through the match), second poll returns "complete" with
    // the credentials the relay minted.
    let server = MockServer::start().await;

    // wiremock matches in declaration order; the first matching mock with
    // remaining `.expect()` budget is consumed. Two single-shot mocks on
    // the same path therefore implement the state transition.
    Mock::given(method("GET"))
        .and(path("/auth/device-code/poll"))
        .and(query_param("code", "dev-XYZ"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "pending",
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/auth/device-code/poll"))
        .and(query_param("code", "dev-XYZ"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "complete",
            "token": "tok-secret",
            "user_id": "01KRB0F61Y6DWMFQG9B4NA6TRN",
            "device_id": "01KRDEVICE0000000000000000",
            "email": "alice@example.com",
            "identity_provider": "github",
            "display_name": "Alice",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let c = client(&server);

    let pending = c
        .poll_device_code(&server.uri(), "dev-XYZ")
        .await
        .expect("first poll succeeds");
    assert_eq!(pending.status, "pending");
    assert!(pending.token.is_none(), "no token on pending");
    assert!(pending.user_id.is_none(), "no user_id on pending");

    let complete = c
        .poll_device_code(&server.uri(), "dev-XYZ")
        .await
        .expect("second poll succeeds");
    assert_eq!(complete.status, "complete");
    assert_eq!(complete.token.as_deref(), Some("tok-secret"));
    assert_eq!(
        complete.user_id.as_deref(),
        Some("01KRB0F61Y6DWMFQG9B4NA6TRN")
    );
    assert_eq!(
        complete.device_id.as_deref(),
        Some("01KRDEVICE0000000000000000")
    );
    assert_eq!(complete.email.as_deref(), Some("alice@example.com"));
    assert_eq!(complete.display_name.as_deref(), Some("Alice"));
}

#[tokio::test]
async fn poll_device_code_denied() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/auth/device-code/poll"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "denied",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let resp = client(&server)
        .poll_device_code(&server.uri(), "dev-XYZ")
        .await
        .expect("poll returns 200 even on denied");
    assert_eq!(resp.status, "denied");
    assert!(resp.token.is_none());
}

#[tokio::test]
async fn poll_device_code_expired() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/auth/device-code/poll"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "status": "expired",
        })))
        .expect(1)
        .mount(&server)
        .await;

    let resp = client(&server)
        .poll_device_code(&server.uri(), "dev-XYZ")
        .await
        .expect("poll returns 200 even on expired");
    assert_eq!(resp.status, "expired");
}
