//! Device and account-management endpoints on `RestClient`.

use reqwest::StatusCode;

use super::{decode_json_response, HttpError, RestClient};
use crate::protocol::DeviceInfo;
use crate::rest::{DeviceRevokeRequest, GetMeRequest, GetMeResponse};

impl RestClient {
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
}
