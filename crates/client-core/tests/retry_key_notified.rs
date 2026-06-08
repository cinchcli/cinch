//! Wiremock unit tests for `RestClient::retry_key_bundle`'s `notified` parsing.
//!
//! `notified` tells `cinch auth retry-key` whether any other device was online
//! to receive the key-exchange re-broadcast, so it can skip the 30s wait when
//! nobody could respond. Older relays omit the field; that must default to
//! `true` so the CLI keeps its previous always-poll behavior.

use client_core::http::RestClient;
use client_core::version::ClientInfo;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn retry_with_body(body: serde_json::Value) -> bool {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/key-bundle/retry"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    let client = RestClient::new(server.uri(), "tok", ClientInfo::for_test()).unwrap();
    client
        .retry_key_bundle()
        .await
        .expect("retry_key_bundle should succeed on 200")
}

#[tokio::test]
async fn notified_true_is_parsed() {
    assert!(retry_with_body(serde_json::json!({"ok": true, "notified": true})).await);
}

#[tokio::test]
async fn notified_false_is_parsed() {
    assert!(!retry_with_body(serde_json::json!({"ok": true, "notified": false})).await);
}

#[tokio::test]
async fn missing_notified_defaults_true_for_old_relays() {
    // An older relay returns just {"ok": true}; we must not fail-fast on it.
    assert!(retry_with_body(serde_json::json!({"ok": true})).await);
}

#[tokio::test]
async fn non_json_body_defaults_true() {
    // A 204 / empty / non-JSON body (e.g. an older relay, or the desktop's
    // mocked 204) must hit the parse-failure fallback, not fail-fast. This is a
    // distinct code path from "valid JSON with the field absent".
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/auth/key-bundle/retry"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&server)
        .await;
    let client = RestClient::new(server.uri(), "tok", ClientInfo::for_test()).unwrap();
    assert!(client
        .retry_key_bundle()
        .await
        .expect("200 should succeed regardless of body"));
}
