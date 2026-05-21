//! Two-device E2EE bootstrap verification.
//!
//! Tests the cryptographic and protocol components that must work together
//! for a joiner device to receive the canonical AES key from a bearer:
//!
//! 1. The ECDH bundle encoding/decoding round-trip (bearer → joiner).
//! 2. The full key-exchange flow: bearer builds bundle, joiner decodes it,
//!    asserts the decrypted bytes equal the canonical key.
//! 3. `register_device_public_key` hitting the right relay endpoint.
//!
//! NOTE: `auth::poll_key_bundle` writes directly to `~/.cinch/config.json`
//! and cannot be called safely in isolated unit tests without a temp config
//! path. Its logic is covered by reading the implementation; the sub-steps
//! it delegates to (derive_shared_key + decrypt) are covered here, and the
//! endpoint integration is covered by the wiremock tests below.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use client_core::{crypto, http::RestClient, key_exchange, version::ClientInfo};
use sha2::{Digest, Sha256};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

/// Simulates the full bearer→joiner ECDH key-bundle round-trip at the
/// cryptographic level. This is the core of what `poll_key_bundle` does
/// internally when it receives a non-empty bundle from the relay.
#[tokio::test]
async fn bearer_to_joiner_ecdh_round_trip_delivers_canonical_key() {
    let canonical = [0x77u8; 32];

    // Joiner: generate a static device keypair (what install_credentials writes).
    let (joiner_priv, joiner_pub) = crypto::generate_device_keypair();

    // Bearer: simulate `key_exchange::respond` — generates ephemeral keypair,
    // derives shared key via HKDF-SHA256(X25519(eph, joiner_pub)), encrypts canonical.
    let (eph_priv, eph_pub) = crypto::generate_ephemeral_keypair();
    let shared_bearer = crypto::derive_shared_key(&eph_priv, &joiner_pub).expect("bearer ECDH");
    let encrypted_bundle =
        crypto::encrypt(&shared_bearer, &canonical).expect("bearer encrypt bundle");

    // Joiner: same HKDF-SHA256(X25519(joiner_priv, eph_pub)) → must equal shared_bearer.
    let shared_joiner = crypto::derive_shared_key(&joiner_priv, &eph_pub).expect("joiner ECDH");
    assert_eq!(
        shared_bearer, shared_joiner,
        "ECDH must be symmetric: X25519(eph,joiner_pub) == X25519(joiner,eph_pub)"
    );

    let plaintext =
        crypto::decrypt(&shared_joiner, &encrypted_bundle).expect("joiner decrypt bundle");
    assert_eq!(
        plaintext, canonical,
        "decrypted bundle must equal original canonical key"
    );
}

/// Confirms that `key_exchange::respond` produces a bundle that decodes to
/// the canonical key — mirroring `key_bearer.rs` but from the joiner's side.
#[tokio::test]
async fn key_exchange_respond_bundle_decodes_to_canonical_key() {
    let canonical = [0xBBu8; 32];
    let canonical_b64 = URL_SAFE_NO_PAD.encode(canonical);

    // Joiner's static keypair — the public key was registered with the relay.
    let (joiner_priv, joiner_pub) = crypto::generate_device_keypair();

    let server = MockServer::start().await;
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<Vec<u8>>));
    let cap = captured.clone();
    Mock::given(method("POST"))
        .and(path("/auth/key-bundle"))
        .respond_with(move |req: &wiremock::Request| {
            *cap.lock().unwrap() = Some(req.body.clone());
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true}))
        })
        .mount(&server)
        .await;

    let client =
        RestClient::new(server.uri(), "token".to_string(), ClientInfo::for_test()).unwrap();

    // Bearer calls respond with the joiner's public key and the canonical key.
    key_exchange::respond(&client, "joiner-device", &joiner_pub, &canonical_b64)
        .await
        .expect("respond ok");

    // Parse what the bearer posted.
    let body = captured.lock().unwrap().clone().expect("body captured");
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let eph_pub = json["ephemeral_public_key"].as_str().expect("eph_pub");
    let enc_bundle = json["encrypted_bundle"].as_str().expect("enc_bundle");

    // Joiner decodes: this is the logic inside poll_key_bundle.
    let shared = crypto::derive_shared_key(&joiner_priv, eph_pub).expect("ECDH");
    let decoded = crypto::decrypt(&shared, enc_bundle).expect("decrypt bundle");

    assert_eq!(
        &decoded[..],
        &canonical[..],
        "joiner must recover canonical key from the bearer's bundle"
    );
}

/// Verifies that `register_device_public_key` posts to the correct relay
/// endpoint with the expected JSON shape.
#[tokio::test]
async fn register_device_public_key_posts_to_correct_endpoint() {
    let server = MockServer::start().await;
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None::<Vec<u8>>));
    let cap = captured.clone();

    Mock::given(method("POST"))
        .and(path("/auth/device/public-key"))
        .respond_with(move |req: &wiremock::Request| {
            *cap.lock().unwrap() = Some(req.body.clone());
            ResponseTemplate::new(204)
        })
        .expect(1) // must be called exactly once
        .mount(&server)
        .await;

    let (_, pub_b64) = crypto::generate_device_keypair();
    let raw_pub = URL_SAFE_NO_PAD.decode(&pub_b64).expect("decode pub");
    let digest = Sha256::digest(&raw_pub);
    let fingerprint: String = digest[..4].iter().map(|b| format!("{:02x}", b)).collect();

    let client =
        RestClient::new(server.uri(), "token".to_string(), ClientInfo::for_test()).unwrap();
    client
        .register_device_public_key(&pub_b64, &fingerprint)
        .await
        .expect("register ok");

    server.verify().await; // assert expect(1) was satisfied

    let body = captured.lock().unwrap().clone().expect("body captured");
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["public_key"].as_str().unwrap(),
        &pub_b64,
        "public_key field must match"
    );
    assert_eq!(
        json["fingerprint"].as_str().unwrap(),
        &fingerprint,
        "fingerprint field must match"
    );
}

/// Confirms that decrypting a clip encrypted by device A with device B's key
/// fails — the other half of why bootstrap matters.
#[test]
fn clip_encrypted_by_device_a_cannot_be_decrypted_by_device_b_without_key_exchange() {
    let key_a = crypto::generate_aes_key();
    let key_b = crypto::generate_aes_key();
    assert_ne!(key_a, key_b);

    let ciphertext = crypto::encrypt(&key_a, b"remote clipboard data").expect("encrypt");
    let result = crypto::decrypt(&key_b, &ciphertext);
    assert!(
        result.is_err(),
        "without key exchange, device B cannot decrypt device A's clips"
    );
}
