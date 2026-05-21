//! Encrypted user-key bundle + ECDH publish/retry endpoints.

use reqwest::StatusCode;

use super::{decode_json_response, HttpError, RestClient};
use crate::rest::{KeyBundlePutRequest, KeyBundleResponse, RegisterDevicePublicKeyRequest};

impl RestClient {
    /// `POST /auth/key-bundle` — publish an encrypted user-key bundle
    /// for `target_device_id`. Called by any device that holds the
    /// user's master key when the relay broadcasts a
    /// `key_exchange_requested` event for a freshly-paired peer.
    /// `ephemeral_public_key` and `encrypted_bundle` are both
    /// base64url-encoded. Bearer-authenticated.
    pub async fn post_key_bundle(
        &self,
        target_device_id: &str,
        ephemeral_public_key: &str,
        encrypted_bundle: &str,
    ) -> Result<(), HttpError> {
        let url = format!("{}/auth/key-bundle", self.base_url);
        let body = KeyBundlePutRequest {
            device_id: target_device_id.to_string(),
            ephemeral_public_key: ephemeral_public_key.to_string(),
            encrypted_bundle: encrypted_bundle.to_string(),
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(HttpError::Unauthorized);
        }
        if !status.is_success() {
            return Err(HttpError::Relay {
                status: status.as_u16(),
                message: format!("post key bundle failed: HTTP {}", status.as_u16()),
                fix: String::new(),
            });
        }
        Ok(())
    }

    /// `POST /auth/device/public-key` — register the X25519 public key
    /// for the calling device so the relay can include it in
    /// ListPendingKeyExchanges sweeps and broadcast
    /// `key_exchange_requested` events for it. Called once after the
    /// OAuth-only login flow finishes installing local credentials.
    /// Bearer-authenticated.
    pub async fn register_device_public_key(
        &self,
        public_key: &str,
        fingerprint: &str,
    ) -> Result<(), HttpError> {
        let url = format!("{}/auth/device/public-key", self.base_url);
        let body = RegisterDevicePublicKeyRequest {
            public_key: public_key.to_string(),
            fingerprint: fingerprint.to_string(),
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(HttpError::Unauthorized);
        }
        if !status.is_success() {
            return Err(HttpError::Relay {
                status: status.as_u16(),
                message: format!("register public key failed: HTTP {}", status.as_u16()),
                fix: String::new(),
            });
        }
        Ok(())
    }

    /// `POST /auth/key-bundle/retry` — ask the relay to re-broadcast
    /// `key_exchange_requested` for the calling device. Used when the
    /// initial key handoff missed (no key-bearer was online at login
    /// time). Bearer-authenticated.
    pub async fn retry_key_bundle(&self) -> Result<(), HttpError> {
        let url = format!("{}/auth/key-bundle/retry", self.base_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(HttpError::Unauthorized);
        }
        if !status.is_success() {
            return Err(HttpError::Relay {
                status: status.as_u16(),
                message: format!("retry key bundle failed: HTTP {}", status.as_u16()),
                fix: String::new(),
            });
        }
        Ok(())
    }

    /// `GET /auth/key-bundle` — fetch the encrypted user-key bundle the
    /// desktop publishes after a pair. Bearer-authenticated.
    /// Always returns 200; an absent bundle is signalled by empty
    /// `ephemeral_public_key`/`encrypted_bundle` plus a non-empty
    /// `pending_since` RFC3339 timestamp, so callers can poll without
    /// distinguishing "not yet" from "device unknown" via status code.
    pub async fn get_key_bundle(&self) -> Result<KeyBundleResponse, HttpError> {
        let url = format!("{}/auth/key-bundle", self.base_url);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;
        decode_json_response::<KeyBundleResponse>(resp).await
    }
}
