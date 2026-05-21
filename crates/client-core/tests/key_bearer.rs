//! Verifies that `key_exchange::respond` produces a bundle the peer
//! can decrypt with its private key.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use client_core::{crypto, http::RestClient, key_exchange, version::ClientInfo};
use wiremock::{matchers::method, matchers::path, Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn responder_posts_bundle_peer_can_decrypt() {
    let (peer_priv_b64, peer_pub_b64) = crypto::generate_ephemeral_keypair();

    let user_key = [0x42u8; 32];
    let user_key_b64 = URL_SAFE_NO_PAD.encode(user_key);

    let server = MockServer::start().await;
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let captured_cl = captured.clone();
    Mock::given(method("POST"))
        .and(path("/auth/key-bundle"))
        .respond_with(move |req: &wiremock::Request| {
            *captured_cl.lock().unwrap() = Some(req.body.clone());
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true}))
        })
        .mount(&server)
        .await;

    let client = RestClient::new(
        server.uri(),
        "test-token".to_string(),
        ClientInfo::for_test(),
    )
    .unwrap();

    key_exchange::respond(&client, "target-dev", &peer_pub_b64, &user_key_b64)
        .await
        .expect("respond ok");

    let body = captured.lock().unwrap().clone().expect("captured");
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let our_eph_pub = json["ephemeral_public_key"].as_str().unwrap();
    let encrypted = json["encrypted_bundle"].as_str().unwrap();

    let shared = crypto::derive_shared_key(&peer_priv_b64, our_eph_pub).unwrap();
    let plaintext = crypto::decrypt(&shared, encrypted).unwrap();
    assert_eq!(&plaintext[..], &user_key[..]);
}
