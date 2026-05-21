//! REST client for the relay's legacy HTTP+JSON endpoints.
//!
//! Targets the same routes the Go CLI uses today: `POST /clips`,
//! `POST /clips/binary`, `GET /clips/latest`, `GET /devices`,
//! `POST /auth/device-code`, `GET /auth/device-code/poll`,
//! `POST /auth/device/revoke`, `POST /auth/key-bundle/retry`.
//! `GET /clips/latest` supports no params (absolute latest), `?source=...`,
//! and `?exclude_source=...` for the three filter modes.
//! The legacy `/auth/pair` and `/auth/pair-token/new` routes were
//! retired in the OAuth-only migration.
//!
//! Retry: 3 attempts with exponential backoff (1s, 2s) matching
//! `cinch/cmd/push.go:188-203`.

use std::time::Duration;

use reqwest::{header::HeaderMap, multipart, Client, StatusCode};

use crate::protocol::{Clip, DeviceInfo};
use crate::rest::{
    DeviceCodeCompleteRequest, DeviceCodeDenyRequest, DeviceCodePollResponse, DeviceCodeRequest,
    DeviceCodeResponse, DeviceRevokeRequest, ErrorResponse, GetMeRequest, GetMeResponse,
    KeyBundlePutRequest, KeyBundleResponse, PushRequest, PushResponse,
    RegisterDevicePublicKeyRequest,
};
use crate::version::ClientInfo;

const MAX_ATTEMPTS: u32 = 3;
const REQUEST_TIMEOUT_SECS: u64 = 30;

/// Filter shape for `RestClient::list_clips`. Mirrors the relay's `ListFilter`.
#[derive(Debug, Default, Clone)]
pub struct ListClipsFilter {
    pub limit: u32,
    pub source: Option<String>,
    pub exclude_source: Option<String>,
    pub exclude_image: bool,
    pub exclude_text: bool,
    pub clip_ids: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("network: {0}")]
    Network(String),
    #[error("auth required (401)")]
    Unauthorized,
    #[error("relay error ({status}): {message}")]
    Relay {
        status: u16,
        message: String,
        fix: String,
    },
    #[error("decode response: {0}")]
    Decode(String),
    #[error("build request: {0}")]
    Build(String),
}

#[cfg(test)]
enum TestMode {
    Offline,
    Recording {
        pushes: std::sync::Arc<std::sync::Mutex<Vec<PushRequest>>>,
        next_seq: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    },
    Scheduled {
        pushes: std::sync::Arc<std::sync::Mutex<Vec<PushRequest>>>,
        schedule: std::sync::Arc<std::sync::Mutex<Vec<FakePush>>>,
        next_seq: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    },
}

#[cfg(test)]
impl std::fmt::Debug for TestMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestMode::Offline => write!(f, "TestMode::Offline"),
            TestMode::Recording { .. } => write!(f, "TestMode::Recording"),
            TestMode::Scheduled { .. } => write!(f, "TestMode::Scheduled"),
        }
    }
}

#[cfg(test)]
impl Clone for TestMode {
    fn clone(&self) -> Self {
        match self {
            TestMode::Offline => TestMode::Offline,
            TestMode::Recording { pushes, next_seq } => TestMode::Recording {
                pushes: std::sync::Arc::clone(pushes),
                next_seq: std::sync::Arc::clone(next_seq),
            },
            TestMode::Scheduled {
                pushes,
                schedule,
                next_seq,
            } => TestMode::Scheduled {
                pushes: std::sync::Arc::clone(pushes),
                schedule: std::sync::Arc::clone(schedule),
                next_seq: std::sync::Arc::clone(next_seq),
            },
        }
    }
}

/// Outcome descriptor for a single simulated push in test clients.
#[cfg(test)]
#[derive(Clone)]
pub enum FakePush {
    Ok,
    Network,
    Relay { status: u16, msg: String },
}

#[derive(Debug, Clone)]
pub struct RestClient {
    base_url: String,
    token: String,
    client: Client,
    client_info: ClientInfo,
    #[cfg(test)]
    test_mode: Option<TestMode>,
}

impl RestClient {
    /// Construct a new client. `relay_url` is trimmed of any trailing slash.
    /// `client_info` is attached to every request as `X-Cinch-Client-Version`
    /// and `X-Cinch-Client-Type` default headers, so the relay's HTTP
    /// middleware can persist the caller's version automatically without each
    /// call site re-setting the headers.
    pub fn new(
        relay_url: impl Into<String>,
        token: impl Into<String>,
        client_info: ClientInfo,
    ) -> Result<Self, HttpError> {
        let base = relay_url.into().trim_end_matches('/').to_string();
        let mut headers = HeaderMap::new();
        for (name, value) in client_info.http_headers() {
            headers.insert(name, value);
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS))
            .default_headers(headers)
            .build()
            .map_err(|e| HttpError::Build(e.to_string()))?;
        Ok(Self {
            base_url: base,
            token: token.into(),
            client,
            client_info,
            #[cfg(test)]
            test_mode: None,
        })
    }

    /// Borrow the `ClientInfo` this client was constructed with. Useful for
    /// callers that also drive a WS connection and want to attach the same
    /// version metadata to the `client_hello` payload.
    pub fn client_info(&self) -> &ClientInfo {
        &self.client_info
    }

    /// Test-only constructor that wires the client to a guaranteed-unreachable
    /// localhost port. Every `push_clip_json` call returns
    /// `Err(HttpError::Network(_))` within milliseconds.
    #[cfg(test)]
    pub fn for_test_offline() -> Self {
        let mut c = Self::new(
            "http://127.0.0.1:1",
            "test-token",
            crate::version::ClientInfo::for_test(),
        )
        .expect("offline test client construction must not fail");
        c.test_mode = Some(TestMode::Offline);
        c
    }

    /// Test-only constructor that records every `push_clip_json` call and
    /// returns a synthetic relay ID for each. Does not touch the network.
    #[cfg(test)]
    pub fn for_test_recording() -> Self {
        Self::for_test_offline().with_mode(TestMode::Recording {
            pushes: Default::default(),
            next_seq: Default::default(),
        })
    }

    /// Test-only constructor with a pre-loaded failure schedule. Each entry in
    /// `schedule` determines the outcome of the corresponding push call in
    /// order; if the schedule is exhausted, subsequent calls return a network
    /// error.
    #[cfg(test)]
    pub fn for_test_with_failures(schedule: Vec<FakePush>) -> Self {
        Self::for_test_offline().with_mode(TestMode::Scheduled {
            pushes: Default::default(),
            schedule: std::sync::Arc::new(std::sync::Mutex::new(schedule)),
            next_seq: Default::default(),
        })
    }

    /// Return the list of push requests recorded by this client (works for
    /// both `Recording` and `Scheduled` modes).
    #[cfg(test)]
    pub fn recorded_pushes(&self) -> Vec<PushRequest> {
        match &self.test_mode {
            Some(TestMode::Recording { pushes, .. }) | Some(TestMode::Scheduled { pushes, .. }) => {
                pushes.lock().unwrap().clone()
            }
            _ => Vec::new(),
        }
    }

    #[cfg(test)]
    fn with_mode(mut self, mode: TestMode) -> Self {
        self.test_mode = Some(mode);
        self
    }

    #[cfg(test)]
    async fn handle_test_push(
        &self,
        mode: &TestMode,
        req: &PushRequest,
    ) -> Result<PushResponse, HttpError> {
        match mode {
            TestMode::Offline => Err(HttpError::Network("offline test client".into())),
            TestMode::Recording { pushes, next_seq } => {
                pushes.lock().unwrap().push(req.clone());
                let n = next_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(PushResponse {
                    clip_id: format!("01HRELAY{:020}", n),
                    byte_size: 0,
                })
            }
            TestMode::Scheduled {
                pushes,
                schedule,
                next_seq,
            } => {
                pushes.lock().unwrap().push(req.clone());
                let fake = {
                    let mut s = schedule.lock().unwrap();
                    if s.is_empty() {
                        return Err(HttpError::Network("schedule exhausted".into()));
                    }
                    s.remove(0)
                };
                match fake {
                    FakePush::Ok => {
                        let n = next_seq.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        Ok(PushResponse {
                            clip_id: format!("01HRELAY{:020}", n),
                            byte_size: 0,
                        })
                    }
                    FakePush::Network => Err(HttpError::Network("fake network".into())),
                    FakePush::Relay { status, msg } => Err(HttpError::Relay {
                        status,
                        message: msg,
                        fix: String::new(),
                    }),
                }
            }
        }
    }

    /// `POST /clips` with JSON body — text and encrypted-binary path.
    pub async fn push_clip_json(&self, req: &PushRequest) -> Result<PushResponse, HttpError> {
        #[cfg(test)]
        {
            if let Some(mode) = &self.test_mode {
                return self.handle_test_push(mode, req).await;
            }
        }
        let url = format!("{}/clips", self.base_url);
        let resp = self
            .send_with_retry(|| {
                self.client
                    .post(&url)
                    .bearer_auth(&self.token)
                    .json(req)
                    .build()
            })
            .await?;
        decode_push_response(resp).await
    }

    /// `POST /clips/binary` — multipart form for unencrypted binary.
    /// `data` is the raw file bytes; metadata fields are sent as form fields.
    pub async fn push_clip_binary(
        &self,
        data: Vec<u8>,
        content_type: &str,
        source: &str,
        label: Option<&str>,
        target_device_id: Option<&str>,
    ) -> Result<PushResponse, HttpError> {
        let url = format!("{}/clips/binary", self.base_url);
        let mut last_err: Option<HttpError> = None;
        for attempt in 0..MAX_ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(1u64 << attempt)).await;
            }
            // Multipart parts must be rebuilt per attempt because their bodies
            // are consumed by `.send()`.
            let mut form = multipart::Form::new()
                .part(
                    "file",
                    multipart::Part::bytes(data.clone()).file_name("upload"),
                )
                .text("content_type", content_type.to_string())
                .text("source", source.to_string());
            if let Some(l) = label.filter(|s| !s.is_empty()) {
                form = form.text("label", l.to_string());
            }
            if let Some(d) = target_device_id.filter(|s| !s.is_empty()) {
                form = form.text("target_device_id", d.to_string());
            }
            let resp = self
                .client
                .post(&url)
                .bearer_auth(&self.token)
                .multipart(form)
                .send()
                .await;
            match resp {
                Ok(r) => return decode_push_response(r).await,
                Err(e) => last_err = Some(HttpError::Network(e.to_string())),
            }
        }
        Err(last_err.unwrap_or(HttpError::Network("max retries exceeded".into())))
    }

    /// `GET /clips/latest?source=...` — most recent clip matching `source`.
    pub async fn get_latest_clip(&self, source: &str) -> Result<Clip, HttpError> {
        let url = format!("{}/clips/latest", self.base_url);
        let resp = self
            .send_with_retry(|| {
                self.client
                    .get(&url)
                    .bearer_auth(&self.token)
                    .query(&[("source", source)])
                    .build()
            })
            .await?;
        decode_json_response::<Clip>(resp).await
    }

    /// `GET /clips/latest` (no params) — most recent clip across all devices.
    pub async fn get_latest_clip_any(&self) -> Result<Clip, HttpError> {
        let url = format!("{}/clips/latest", self.base_url);
        let resp = self
            .send_with_retry(|| self.client.get(&url).bearer_auth(&self.token).build())
            .await?;
        decode_json_response::<Clip>(resp).await
    }

    /// `GET /clips/{id}/media` — raw image bytes for image clips.
    pub async fn get_clip_media(&self, clip_id: &str) -> Result<Vec<u8>, HttpError> {
        let url = format!("{}/clips/{}/media", self.base_url, clip_id);
        let resp = self
            .send_with_retry(|| self.client.get(&url).bearer_auth(&self.token).build())
            .await?;
        let status = resp.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(HttpError::Unauthorized);
        }
        if !status.is_success() {
            return Err(HttpError::Relay {
                status: status.as_u16(),
                message: format!("Image not found on relay (HTTP {}).", status.as_u16()),
                fix: String::new(),
            });
        }
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| HttpError::Decode(e.to_string()))
    }

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

    /// `POST /auth/display-name` — update `users.display_name` for the
    /// calling user. Returns the stored value after server-side trim.
    /// Empty input is rejected client-side without a network round trip.
    /// Bearer-authenticated.
    pub async fn set_display_name(&self, name: &str) -> Result<String, HttpError> {
        if name.trim().is_empty() {
            return Err(HttpError::Relay {
                status: 400,
                message: "display_name must not be empty".into(),
                fix: "Pass a non-empty name.".into(),
            });
        }
        #[derive(serde::Serialize)]
        struct Req<'a> {
            display_name: &'a str,
        }
        #[derive(serde::Deserialize)]
        struct Resp {
            #[serde(default)]
            display_name: String,
        }
        let url = format!("{}/auth/display-name", self.base_url);
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&Req { display_name: name })
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
                message: format!("set_display_name failed: HTTP {}", status.as_u16()),
                fix: String::new(),
            });
        }
        let body: Resp = resp
            .json()
            .await
            .map_err(|e| HttpError::Network(format!("decode set_display_name response: {}", e)))?;
        Ok(body.display_name)
    }

    /// `POST /auth/device/revoke` — revoke the active device server-side.
    /// Best-effort: callers should still wipe local credentials regardless
    /// of relay reachability.
    pub async fn revoke_device(&self, device_id: &str) -> Result<(), HttpError> {
        let url = format!("{}/auth/device/revoke", self.base_url);
        let body = DeviceRevokeRequest {
            device_id: device_id.to_string(),
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
        if !status.is_success() {
            return Err(HttpError::Relay {
                status: status.as_u16(),
                message: format!("revoke failed: HTTP {}", status.as_u16()),
                fix: String::new(),
            });
        }
        Ok(())
    }

    /// `PUT /devices/{device_id}/nickname` — set or clear a human-readable
    /// nickname for a paired device. An empty string clears the nickname.
    /// Task 5.9 uses this path; the desktop `set_device_nickname` command
    /// delegates here rather than calling reqwest directly.
    pub async fn set_device_nickname(
        &self,
        device_id: &str,
        nickname: &str,
    ) -> Result<(), HttpError> {
        let url = format!("{}/devices/{}/nickname", self.base_url, device_id);
        #[derive(serde::Serialize)]
        struct NicknameBody<'a> {
            nickname: &'a str,
        }
        let resp = self
            .client
            .put(&url)
            .bearer_auth(&self.token)
            .json(&NicknameBody { nickname })
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(HttpError::Relay {
                status: status.as_u16(),
                message: format!("set_device_nickname failed: {}", body),
                fix: String::new(),
            });
        }
        Ok(())
    }

    /// `PUT /devices/self/retention` — set this device's remote retention
    /// (in days). The relay only exposes a self-targeted endpoint; per-device
    /// retention writes are not supported over REST.
    pub async fn set_remote_retention(&self, days: i32) -> Result<(), HttpError> {
        let url = format!("{}/devices/self/retention", self.base_url);
        #[derive(serde::Serialize)]
        struct Body {
            remote_retention_days: i32,
        }
        let resp = self
            .client
            .put(&url)
            .bearer_auth(&self.token)
            .json(&Body {
                remote_retention_days: days,
            })
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(HttpError::Relay {
                status: status.as_u16(),
                message: format!("set_remote_retention failed: {}", body),
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

    /// `GET /clips[?since=<rfc3339>][&limit=<n>]` — list clips, optionally filtered to those
    /// newer than `since`. Returns oldest-first when `since` is provided.
    /// `limit` caps the number of results (relay maximum is 100).
    pub async fn list_clips_since(
        &self,
        since: Option<chrono::DateTime<chrono::Utc>>,
        limit: u32,
    ) -> Result<Vec<Clip>, HttpError> {
        let url = format!("{}/clips", self.base_url);
        let resp = self
            .send_with_retry(|| {
                let mut req = self.client.get(&url).bearer_auth(&self.token);
                if let Some(ts) = since {
                    req = req.query(&[("since", ts.to_rfc3339())]);
                }
                req = req.query(&[("limit", limit.to_string())]);
                req.build()
            })
            .await?;
        decode_json_response::<Vec<Clip>>(resp).await
    }

    /// `GET /clips?...` — list clips with the given filter, newest-first.
    /// Limit is clamped server-side; the client clamps to 200 to match the relay cap.
    pub async fn list_clips(&self, filter: ListClipsFilter) -> Result<Vec<Clip>, HttpError> {
        let url = format!("{}/clips", self.base_url);
        let resp = self
            .send_with_retry(|| {
                let mut req = self.client.get(&url).bearer_auth(&self.token);
                let limit = if filter.limit == 0 {
                    50
                } else {
                    filter.limit.min(200)
                };
                req = req.query(&[("limit", limit.to_string())]);
                if let Some(s) = &filter.source {
                    req = req.query(&[("source", s.as_str())]);
                }
                if let Some(s) = &filter.exclude_source {
                    req = req.query(&[("exclude_source", s.as_str())]);
                }
                if filter.exclude_image {
                    req = req.query(&[("exclude_image", "true")]);
                }
                if filter.exclude_text {
                    req = req.query(&[("exclude_text", "true")]);
                }
                for id in &filter.clip_ids {
                    req = req.query(&[("clip_id", id.as_str())]);
                }
                req.build()
            })
            .await?;
        decode_json_response::<Vec<Clip>>(resp).await
    }

    /// `GET /clips?clip_id=<id>&limit=1` — fetch one clip by ID.
    pub async fn get_clip_by_id(&self, clip_id: &str) -> Result<Clip, HttpError> {
        let clips = self
            .list_clips(ListClipsFilter {
                limit: 1,
                clip_ids: vec![clip_id.to_string()],
                ..Default::default()
            })
            .await?;
        clips.into_iter().next().ok_or_else(|| HttpError::Relay {
            status: 404,
            message: format!("Clip {} not found.", clip_id),
            fix: String::new(),
        })
    }

    /// `GET /clips/latest?exclude_source=<key>` — latest clip whose source != exclude_source.
    pub async fn get_latest_clip_excluding(&self, exclude_source: &str) -> Result<Clip, HttpError> {
        let url = format!("{}/clips/latest", self.base_url);
        let resp = self
            .send_with_retry(|| {
                self.client
                    .get(&url)
                    .bearer_auth(&self.token)
                    .query(&[("exclude_source", exclude_source)])
                    .build()
            })
            .await?;
        decode_json_response::<Clip>(resp).await
    }

    /// `DELETE /clips/{id}` — remove a clip. 404 is treated as success.
    pub async fn delete_clip(&self, clip_id: &str) -> Result<(), HttpError> {
        let url = format!("{}/clips/{}", self.base_url, clip_id);
        let resp = self
            .send_with_retry(|| self.client.delete(&url).bearer_auth(&self.token).build())
            .await?;
        let status = resp.status();
        if status == StatusCode::NOT_FOUND || status.is_success() {
            return Ok(());
        }
        if status == StatusCode::UNAUTHORIZED {
            return Err(HttpError::Unauthorized);
        }
        Err(HttpError::Relay {
            status: status.as_u16(),
            message: format!("Delete clip failed (HTTP {}).", status.as_u16()),
            fix: String::new(),
        })
    }

    /// `POST /clips/{id}/pin` — set or clear pin state. Best-effort: 404 treated as success.
    pub async fn set_clip_pin(
        &self,
        clip_id: &str,
        is_pinned: bool,
        pin_note: Option<&str>,
    ) -> Result<(), HttpError> {
        let url = format!("{}/clips/{}/pin", self.base_url, clip_id);
        #[derive(serde::Serialize)]
        struct PinBody<'a> {
            is_pinned: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            pin_note: Option<&'a str>,
        }
        let body = PinBody {
            is_pinned,
            pin_note,
        };
        let resp = self
            .send_with_retry(|| {
                self.client
                    .post(&url)
                    .bearer_auth(&self.token)
                    .json(&body)
                    .build()
            })
            .await?;
        let status = resp.status();
        if status == StatusCode::NOT_FOUND || status.is_success() {
            return Ok(());
        }
        if status == StatusCode::UNAUTHORIZED {
            return Err(HttpError::Unauthorized);
        }
        Err(HttpError::Relay {
            status: status.as_u16(),
            message: format!("Set clip pin failed (HTTP {}).", status.as_u16()),
            fix: String::new(),
        })
    }

    /// `GET /devices` — list of paired devices for the current user.
    pub async fn list_devices(&self) -> Result<Vec<DeviceInfo>, HttpError> {
        let url = format!("{}/devices", self.base_url);
        let resp = self
            .send_with_retry(|| self.client.get(&url).bearer_auth(&self.token).build())
            .await?;
        decode_json_response::<Vec<DeviceInfo>>(resp).await
    }

    /// `POST /cinch.v1.MeService/GetMe` (Connect-RPC unary, JSON encoding)
    /// — fetch the caller's plan tier + active usage. Read-only; plan
    /// changes go through ops, not the API.
    pub async fn get_me(&self) -> Result<GetMeResponse, HttpError> {
        let url = format!("{}/cinch.v1.MeService/GetMe", self.base_url);
        let body = GetMeRequest {};
        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .map_err(|e| HttpError::Network(e.to_string()))?;
        decode_json_response::<GetMeResponse>(resp).await
    }

    async fn send_with_retry<F>(&self, build: F) -> Result<reqwest::Response, HttpError>
    where
        F: Fn() -> Result<reqwest::Request, reqwest::Error>,
    {
        let mut last_err: Option<HttpError> = None;
        for attempt in 0..MAX_ATTEMPTS {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(1u64 << attempt)).await;
            }
            let req = build().map_err(|e| HttpError::Build(e.to_string()))?;
            match self.client.execute(req).await {
                Ok(resp) => return Ok(resp),
                Err(e) => last_err = Some(HttpError::Network(e.to_string())),
            }
        }
        Err(last_err.unwrap_or(HttpError::Network("max retries exceeded".into())))
    }
}

async fn decode_push_response(resp: reqwest::Response) -> Result<PushResponse, HttpError> {
    decode_json_response::<PushResponse>(resp).await
}

async fn decode_json_response<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
) -> Result<T, HttpError> {
    let status = resp.status();
    if status == StatusCode::UNAUTHORIZED {
        return Err(HttpError::Unauthorized);
    }
    if !status.is_success() {
        let err: ErrorResponse = resp.json().await.unwrap_or_default();
        let message = if !err.message.is_empty() {
            err.message
        } else {
            err.error
        };
        return Err(HttpError::Relay {
            status: status.as_u16(),
            message,
            fix: err.fix,
        });
    }
    resp.json::<T>()
        .await
        .map_err(|e| HttpError::Decode(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{HttpError, RestClient};
    use crate::proto::cinch::v1::DeviceCodeStartRequest;
    use crate::version::ClientInfo;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn device_code_start_request_includes_user_hint_when_set() {
        let req = DeviceCodeStartRequest {
            hostname: Some("dev-box-3".into()),
            machine_id: Some("m1".into()),
            user_hint: Some("alice@example.com".into()),
        };
        let bytes = serde_json::to_vec(&req).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["user_hint"], "alice@example.com");
    }

    #[test]
    fn device_code_start_request_omits_user_hint_when_none() {
        let req = DeviceCodeStartRequest {
            hostname: Some("dev-box-3".into()),
            machine_id: Some("m1".into()),
            user_hint: None,
        };
        let bytes = serde_json::to_vec(&req).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            parsed.get("user_hint").is_none(),
            "user_hint must omit when None"
        );
    }

    #[tokio::test]
    async fn set_display_name_happy_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/display-name"))
            .and(header("authorization", "Bearer testtok"))
            .and(body_json(serde_json::json!({"display_name": "Alice"})))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"ok": true, "display_name": "Alice"})),
            )
            .mount(&server)
            .await;

        let client = RestClient::new(server.uri(), "testtok", ClientInfo::for_test()).unwrap();

        let stored = client.set_display_name("Alice").await.expect("ok");
        assert_eq!(stored, "Alice");
    }

    #[tokio::test]
    async fn set_display_name_rejects_empty() {
        let client = RestClient::new("http://unused", "t", ClientInfo::for_test()).unwrap();
        let err = client.set_display_name("").await.expect_err("must reject");
        assert!(matches!(err, HttpError::Relay { status: 400, .. }));
    }

    #[tokio::test]
    async fn set_display_name_propagates_unauthorized() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/auth/display-name"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&server)
            .await;
        let client = RestClient::new(server.uri(), "t", ClientInfo::for_test()).unwrap();
        let err = client.set_display_name("Bob").await.expect_err("401");
        assert!(matches!(err, HttpError::Unauthorized));
    }
}
