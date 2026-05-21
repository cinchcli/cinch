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
//!
//! The methods themselves are split across feature-grouped submodules
//! (`auth`, `clips`, `devices`, `key_bundle`); this file owns the
//! shared `RestClient` struct, error types, retry plumbing, and JSON
//! decoders that every endpoint reuses.

use std::time::Duration;

use reqwest::{header::HeaderMap, Client, StatusCode};

#[cfg(test)]
use crate::rest::PushRequest;
use crate::rest::{ErrorResponse, PushResponse};
use crate::version::ClientInfo;

mod auth;
mod clips;
mod devices;
mod key_bundle;

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
