#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// run_joiner_flow_with_privkey posts to /auth/device/public-key exactly once
    /// and then polls /auth/key-bundle. With an empty bundle response the function
    /// completes without writing a key (no-bearer path) — but the HTTP calls must
    /// still happen.
    #[tokio::test]
    async fn joiner_flow_registers_pubkey_and_polls_bundle() {
        let server = MockServer::start().await;

        // Must receive exactly one POST /auth/device/public-key
        Mock::given(method("POST"))
            .and(path("/auth/device/public-key"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        // GET /auth/key-bundle — returns empty bundle (no bearer online)
        Mock::given(method("GET"))
            .and(path("/auth/key-bundle"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ephemeral_public_key": "",
                "encrypted_bundle": "",
                "pending_since": "2026-05-08T00:00:00Z"
            })))
            .expect(1..)
            .mount(&server)
            .await;

        // Use a known X25519 keypair so we don't touch the real credstore.
        let (priv_b64, _pub_b64) = client_core::crypto::generate_device_keypair();

        crate::auth_bootstrap::run_joiner_flow_with_privkey(
            &server.uri(),
            "test-token",
            &priv_b64,
            "test-user",
        )
        .await;

        // wiremock asserts expect(1) on the public-key endpoint was satisfied.
        server.verify().await;
    }

    /// When the relay replies with a valid ECDH bundle the joiner decrypts the
    /// canonical key. Verify register_device_public_key is still called first.
    #[tokio::test]
    async fn joiner_flow_with_bundle_calls_both_endpoints() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;

        let server = MockServer::start().await;

        // Generate the joiner's keypair.
        let (joiner_priv, joiner_pub) = client_core::crypto::generate_device_keypair();

        // Bearer side: build a valid ECDH bundle so poll_key_bundle returns true.
        let canonical = [0xABu8; 32];
        let (eph_priv, eph_pub) = client_core::crypto::generate_ephemeral_keypair();
        let shared = client_core::crypto::derive_shared_key(&eph_priv, &joiner_pub).expect("ECDH");
        let encrypted_bundle =
            client_core::crypto::encrypt(&shared, &canonical).expect("encrypt bundle");

        Mock::given(method("POST"))
            .and(path("/auth/device/public-key"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/auth/key-bundle"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ephemeral_public_key": eph_pub,
                "encrypted_bundle": encrypted_bundle,
            })))
            .expect(1..)
            .mount(&server)
            .await;

        crate::auth_bootstrap::run_joiner_flow_with_privkey(
            &server.uri(),
            "test-token",
            &joiner_priv,
            "test-user-bundle",
        )
        .await;

        server.verify().await;

        // Sanity-check: the joiner can independently derive the same shared key
        // and decrypt the bundle — this is what poll_key_bundle did internally.
        let shared_joiner =
            client_core::crypto::derive_shared_key(&joiner_priv, &eph_pub).expect("ECDH joiner");
        assert_eq!(shared, shared_joiner, "ECDH must be symmetric");
        let _ = URL_SAFE_NO_PAD
            .decode(&joiner_pub)
            .expect("pub is valid base64");
    }
}
