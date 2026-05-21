//! Device-code login flow + health probe.
//!
//! These endpoints don't share the `send_with_retry` plumbing because
//! they're driven by the caller's polling loop, not by transient
//! network retries.

use super::{decode_json_response, HttpError, RestClient};
use crate::rest::{
    DeviceCodeCompleteRequest, DeviceCodeDenyRequest, DeviceCodePollResponse, DeviceCodeRequest,
    DeviceCodeResponse,
};

impl RestClient {
    /// `POST /auth/device-code` — start the device-code flow. The relay
    /// returns a `verification_uri` for the user to open in a browser.
    /// `machine_id` is opaque (empty string disables relay-side dedup).
    pub async fn start_device_code(
        &self,
        relay_url: &str,
        hostname: &str,
        machine_id: &str,
        user_hint: Option<&str>,
    ) -> Result<DeviceCodeResponse, HttpError> {
        let url = format!("{}/auth/device-code", relay_url.trim_end_matches('/'));
        let req = DeviceCodeRequest {
            hostname: Some(hostname.to_string()),
            machine_id: if machine_id.is_empty() {
                None
            } else {
                Some(machine_id.to_string())
            },
            user_hint: user_hint.map(|s| s.to_string()),
        };
        let resp = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;
        decode_json_response::<DeviceCodeResponse>(resp).await
    }

    /// `GET /auth/device-code/poll?code=...` — single poll. Caller drives
    /// the loop and respects `interval` from the start response.
    pub async fn poll_device_code(
        &self,
        relay_url: &str,
        device_code: &str,
    ) -> Result<DeviceCodePollResponse, HttpError> {
        let url = format!("{}/auth/device-code/poll", relay_url.trim_end_matches('/'));
        let resp = self
            .client
            .get(&url)
            .query(&[("code", device_code)])
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;
        decode_json_response::<DeviceCodePollResponse>(resp).await
    }

    /// `POST /auth/device-code/complete` — approve a pending device-code
    /// login from an already-authenticated local device.
    pub async fn complete_device_code(&self, user_code: &str) -> Result<(), HttpError> {
        let url = format!("{}/auth/device-code/complete", self.base_url);
        let body = DeviceCodeCompleteRequest {
            user_code: user_code.to_string(),
            user_id: String::new(),
            device_id: String::new(),
            token: String::new(),
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;
        decode_json_response::<serde_json::Value>(resp)
            .await
            .map(|_| ())
    }

    /// `POST /cinch.v1.AuthService/DeviceCodeDeny` (Connect-RPC unary, JSON encoding)
    /// — reject a pending device-code login from this already-signed-in device.
    pub async fn deny_device_code(&self, user_code: &str) -> Result<(), HttpError> {
        let url = format!("{}/cinch.v1.AuthService/DeviceCodeDeny", self.base_url);
        let body = DeviceCodeDenyRequest {
            user_code: user_code.to_string(),
        };
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;
        decode_json_response::<serde_json::Value>(resp)
            .await
            .map(|_| ())
    }

    /// `GET /health` — liveness probe used by the wizard before issuing a
    /// device code, so URL typos surface as a clean error before the user
    /// is sent to a browser.
    pub async fn probe_relay(&self, relay_url: &str) -> Result<(), HttpError> {
        let url = format!("{}/health", relay_url.trim_end_matches('/'));
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(HttpError::Relay {
                status: resp.status().as_u16(),
                message: format!("health check failed: HTTP {}", resp.status().as_u16()),
                fix: String::new(),
            })
        }
    }
}
