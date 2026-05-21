//! PostHog HTTP backend. Vendor-specific code is confined to this file.
//!
//! Posts buffered events to `<TELEMETRY_URL>/batch/` as PostHog's documented
//! capture batch payload. Failures are silently dropped — telemetry must
//! never affect the main flow.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Map, Value};

use super::event::Event;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const APP_SURFACE: &str = "cli";
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

pub struct PostHogBackend {
    url: String,
    api_key: String,
    distinct_id: Arc<Mutex<String>>,
    buffer: Arc<Mutex<Vec<BatchEntry>>>,
    http: reqwest::Client,
}

#[derive(Serialize, Debug, Clone)]
struct BatchEntry {
    event: String,
    distinct_id: String,
    properties: Map<String, Value>,
    timestamp: String,
}

#[derive(Serialize)]
struct BatchPayload {
    api_key: String,
    batch: Vec<BatchEntry>,
}

impl PostHogBackend {
    pub fn new(url: &str, api_key: &str, distinct_id: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            url: url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            distinct_id: Arc::new(Mutex::new(distinct_id)),
            buffer: Arc::new(Mutex::new(Vec::new())),
            http,
        }
    }

    fn build_entry(
        &self,
        name: String,
        mut properties: Map<String, Value>,
        distinct_id_override: Option<String>,
    ) -> BatchEntry {
        properties.insert("app".into(), Value::String(APP_SURFACE.into()));
        properties.insert("app_version".into(), Value::String(APP_VERSION.into()));
        properties.insert("os".into(), Value::String(std::env::consts::OS.into()));
        properties.insert("arch".into(), Value::String(std::env::consts::ARCH.into()));
        // Empty `$ip` is client-side defense-in-depth; the primary IP-off
        // control lives in the PostHog project settings.
        properties.insert("$ip".into(), Value::String(String::new()));
        let distinct_id = distinct_id_override.unwrap_or_else(|| {
            self.distinct_id
                .lock()
                .map(|g| g.clone())
                .unwrap_or_default()
        });
        BatchEntry {
            event: name,
            distinct_id,
            properties,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn capture(&self, event: Event) {
        let entry = self.build_entry(event.name, event.properties, None);
        if let Ok(mut buf) = self.buffer.lock() {
            buf.push(entry);
        }
    }

    pub fn identify(&self, user_id: &str) {
        let anon_id = self
            .distinct_id
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default();
        if anon_id == user_id {
            return;
        }
        let mut props = Map::new();
        props.insert("$anon_distinct_id".into(), Value::String(anon_id));
        let entry = self.build_entry("$identify".to_string(), props, Some(user_id.to_string()));
        if let Ok(mut buf) = self.buffer.lock() {
            buf.push(entry);
        }
        if let Ok(mut id) = self.distinct_id.lock() {
            *id = user_id.to_string();
        }
    }

    pub async fn flush(&self) {
        let batch: Vec<BatchEntry> = match self.buffer.lock() {
            Ok(mut buf) => std::mem::take(&mut *buf),
            Err(_) => return,
        };
        if batch.is_empty() {
            return;
        }
        let payload = BatchPayload {
            api_key: self.api_key.clone(),
            batch,
        };
        let url = format!("{}/batch/", self.url);
        let _ = self.http.post(&url).json(&payload).send().await;
    }
}
