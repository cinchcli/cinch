//! Clip push + read endpoints on `RestClient`.

use std::time::Duration;

use reqwest::{multipart, StatusCode};

use super::{
    decode_json_response, decode_push_response, HttpError, ListClipsFilter, RestClient,
    MAX_ATTEMPTS,
};
use crate::protocol::Clip;
use crate::rest::{PushRequest, PushResponse};

impl RestClient {
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
}
