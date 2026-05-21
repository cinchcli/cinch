//! Desktop joiner flow: register X25519 pubkey + poll for canonical AES key.
//!
//! Mirrors CLI's post-install bootstrap (cinch/crates/cli/src/commands/auth.rs:266-336).
//! Called after install_credentials in sign_in and pair_with_token; runs concurrently
//! with the WS client so the WS connection is not delayed. If a bearer responds within
//! 30s, the canonical key overwrites the locally-generated placeholder and future
//! decrypts succeed. On timeout, decrypt failures trigger auto-retry via the WS path.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use sha2::{Digest, Sha256};

pub async fn run_joiner_flow(relay_url: &str, token: &str, user_id: &str, device_id: &str) {
    let priv_b64 = match client_core::credstore::read_device_privkey(user_id, device_id) {
        Some(k) => k,
        None => {
            log::error!(
                "auth_bootstrap: no device private key for {}/{} — skipping",
                user_id,
                device_id
            );
            return;
        }
    };
    run_joiner_flow_with_privkey(relay_url, token, &priv_b64, user_id).await;
}

/// Inner function that receives the already-read private key. Separated so
/// tests can inject a known keypair without touching the real credstore.
pub(crate) async fn run_joiner_flow_with_privkey(
    relay_url: &str,
    token: &str,
    priv_b64: &str,
    user_id: &str,
) {
    let pub_b64 = match client_core::crypto::pub_from_priv(priv_b64) {
        Ok(p) => p,
        Err(e) => {
            log::error!("auth_bootstrap: derive pubkey failed: {}", e);
            return;
        }
    };

    let raw_pub = match URL_SAFE_NO_PAD.decode(&pub_b64) {
        Ok(b) => b,
        Err(e) => {
            log::error!("auth_bootstrap: decode pubkey failed: {}", e);
            return;
        }
    };
    let fingerprint: String = Sha256::digest(&raw_pub)[..4]
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect();

    let client = match client_core::http::RestClient::new(
        relay_url.to_string(),
        token.to_string(),
        crate::build_client_info(),
    ) {
        Ok(c) => c,
        Err(e) => {
            log::error!("auth_bootstrap: build client failed: {}", e);
            return;
        }
    };

    if let Err(e) = client
        .register_device_public_key(&pub_b64, &fingerprint)
        .await
    {
        // Best-effort: a network blip here should not block onboarding.
        log::warn!("auth_bootstrap: register_device_public_key failed: {}", e);
    }

    let got_key = client_core::auth::poll_key_bundle(&client, priv_b64, user_id).await;
    if got_key {
        log::info!("auth_bootstrap: canonical key received from bearer");
    } else {
        log::warn!(
            "auth_bootstrap: no bearer responded within 30s; \
            placeholder key in place — decrypt failures will trigger retry"
        );
    }
}
