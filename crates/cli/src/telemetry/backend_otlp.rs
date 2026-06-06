//! OTLP-via-relay HTTP backend. Transport specifics are confined to this file.
//!
//! Posts buffered events to `<relay_base>/telemetry/otlp` as a small JSON batch
//! where every attribute value is a string (the relay re-emits them as OTLP log
//! attributes after HMAC-ing the anonymous id). Failures are silently dropped —
//! telemetry must never affect the main flow.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use super::event::Event;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const APP_SURFACE: &str = "cli";
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

/// One attribute on the wire. Both key and value are strings — the relay treats
/// the batch as untyped string attributes and coerces upstream if needed.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
struct OtlpAttr {
    k: String,
    v: String,
}

/// A single event: a name plus its flattened string attributes.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
struct OtlpEvent {
    name: String,
    attrs: Vec<OtlpAttr>,
}

/// The POST body. `anon_id` is the raw client-generated UUID; the relay HMACs it
/// before forwarding, so sending it raw here is intentional and not identity.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
struct OtlpBatch {
    anon_id: String,
    events: Vec<OtlpEvent>,
}

pub struct OtlpBackend {
    relay_base: String,
    anon_id: String,
    buffer: Arc<Mutex<Vec<Event>>>,
    http: reqwest::Client,
}

impl OtlpBackend {
    pub fn new(relay_base: String, anon_id: String) -> Self {
        let http = reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            relay_base,
            anon_id,
            buffer: Arc::new(Mutex::new(Vec::new())),
            http,
        }
    }

    pub fn capture(&self, event: Event) {
        if let Ok(mut buf) = self.buffer.lock() {
            buf.push(event);
        }
    }

    pub async fn flush(&self) {
        let events: Vec<Event> = match self.buffer.lock() {
            Ok(mut buf) => std::mem::take(&mut *buf),
            Err(_) => return,
        };
        if events.is_empty() {
            return;
        }
        // No destination configured (opted-in user not signed in to any relay):
        // there is nothing to send to, so drop the drained events.
        if self.relay_base.is_empty() {
            return;
        }

        let batch = build_batch(&self.anon_id, events);
        let url = format!("{}/telemetry/otlp", self.relay_base.trim_end_matches('/'));
        let user_agent = format!("cinch-cli/{}", APP_VERSION);
        let _ = self
            .http
            .post(&url)
            .header(reqwest::header::USER_AGENT, user_agent)
            .json(&batch)
            .send()
            .await;
    }
}

/// Coerces a single property value to its string wire form.
///
/// Returns `None` for `Null` so callers drop the attribute entirely.
fn coerce_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        Value::Bool(b) => Some(if *b { "true" } else { "false" }.to_string()),
        Value::Number(n) => Some(n.to_string()),
        // Arrays/objects are serialized compactly; serde_json never fails on a
        // valid Value, but fall back to dropping the attr if it somehow does.
        other => serde_json::to_string(other).ok(),
    }
}

/// Builds the OTLP batch payload from raw events, coercing every property to a
/// string and injecting the coarse client dimensions (app/version/os/arch).
fn build_batch(anon_id: &str, events: Vec<Event>) -> OtlpBatch {
    let otlp_events = events
        .into_iter()
        .map(|event| {
            let mut attrs: Vec<OtlpAttr> = event
                .properties
                .iter()
                .filter_map(|(k, v)| coerce_value(v).map(|v| OtlpAttr { k: k.clone(), v }))
                .collect();
            // Injected dimensions are attrs, not properties. No `$ip` — the relay
            // is the only place a network identifier could be observed.
            attrs.push(OtlpAttr {
                k: "app".to_string(),
                v: APP_SURFACE.to_string(),
            });
            attrs.push(OtlpAttr {
                k: "app_version".to_string(),
                v: APP_VERSION.to_string(),
            });
            attrs.push(OtlpAttr {
                k: "os".to_string(),
                v: std::env::consts::OS.to_string(),
            });
            attrs.push(OtlpAttr {
                k: "arch".to_string(),
                v: std::env::consts::ARCH.to_string(),
            });
            OtlpEvent {
                name: event.name,
                attrs,
            }
        })
        .collect();
    OtlpBatch {
        anon_id: anon_id.to_string(),
        events: otlp_events,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_ID: &str = "0190b8e0-1234-7abc-89de-f0123456789a";

    fn attr<'a>(attrs: &'a [OtlpAttr], key: &str) -> Option<&'a str> {
        attrs.iter().find(|a| a.k == key).map(|a| a.v.as_str())
    }

    #[test]
    fn coerces_property_types_to_strings() {
        let event = Event::new("cli.command.completed")
            .with("command", "send")
            .with("success", true)
            .with("failed", false)
            .with("duration_ms", 1234_u64)
            .with("exit_code", -1_i32);
        let batch = build_batch(SAMPLE_ID, vec![event]);
        let attrs = &batch.events[0].attrs;
        assert_eq!(attr(attrs, "command"), Some("send"));
        assert_eq!(attr(attrs, "success"), Some("true"));
        assert_eq!(attr(attrs, "failed"), Some("false"));
        assert_eq!(attr(attrs, "duration_ms"), Some("1234"));
        assert_eq!(attr(attrs, "exit_code"), Some("-1"));
    }

    #[test]
    fn serializes_nested_object_as_compact_json() {
        let event = Event::new("e").with("meta", serde_json::json!({"a": 1, "b": [2, 3]}));
        let batch = build_batch(SAMPLE_ID, vec![event]);
        let attrs = &batch.events[0].attrs;
        // serde_json::Map preserves insertion order with the default features,
        // so the compact form is deterministic here.
        assert_eq!(attr(attrs, "meta"), Some(r#"{"a":1,"b":[2,3]}"#));
    }

    #[test]
    fn serializes_array_as_compact_json() {
        let event = Event::new("e").with("tags", serde_json::json!(["x", "y"]));
        let batch = build_batch(SAMPLE_ID, vec![event]);
        let attrs = &batch.events[0].attrs;
        assert_eq!(attr(attrs, "tags"), Some(r#"["x","y"]"#));
    }

    #[test]
    fn null_property_is_dropped() {
        let event = Event::new("e")
            .with("nothing", Value::Null)
            .with("keep", "yes");
        let batch = build_batch(SAMPLE_ID, vec![event]);
        let attrs = &batch.events[0].attrs;
        assert_eq!(attr(attrs, "nothing"), None);
        assert_eq!(attr(attrs, "keep"), Some("yes"));
    }

    #[test]
    fn injects_app_dimensions_and_omits_ip() {
        let batch = build_batch(SAMPLE_ID, vec![Event::new("e")]);
        let attrs = &batch.events[0].attrs;
        assert_eq!(attr(attrs, "app"), Some("cli"));
        assert_eq!(attr(attrs, "app_version"), Some(APP_VERSION));
        assert_eq!(attr(attrs, "os"), Some(std::env::consts::OS));
        assert_eq!(attr(attrs, "arch"), Some(std::env::consts::ARCH));
        assert_eq!(attr(attrs, "$ip"), None);
    }

    #[test]
    fn does_not_carry_clipboard_content() {
        // Defense-in-depth: callers never set content, but assert the builder
        // does not invent any content-bearing key.
        let batch = build_batch(SAMPLE_ID, vec![Event::new("cli.command.completed")]);
        let attrs = &batch.events[0].attrs;
        for forbidden in ["content", "clip", "text", "payload", "body"] {
            assert_eq!(
                attr(attrs, forbidden),
                None,
                "key {forbidden} must not appear"
            );
        }
    }

    #[test]
    fn preserves_anon_id_and_event_name() {
        let batch = build_batch(SAMPLE_ID, vec![Event::new("cli.command.invoked")]);
        assert_eq!(batch.anon_id, SAMPLE_ID);
        assert_eq!(batch.events[0].name, "cli.command.invoked");
    }

    #[test]
    fn serialized_payload_matches_wire_contract_shape() {
        let event = Event::new("cli.command.completed")
            .with("command", "send")
            .with("success", true)
            .with("duration_ms", 1234_u64);
        let batch = build_batch(SAMPLE_ID, vec![event]);
        let json = serde_json::to_value(&batch).expect("serialize batch");
        // Top-level keys exactly as the contract specifies.
        assert!(json.get("anon_id").is_some());
        assert!(json.get("events").is_some());
        assert_eq!(json["anon_id"], SAMPLE_ID);
        let first = &json["events"][0];
        assert_eq!(first["name"], "cli.command.completed");
        // Each attr is a {k, v} object with string values.
        let attrs = first["attrs"].as_array().expect("attrs is array");
        for a in attrs {
            assert!(a.get("k").and_then(Value::as_str).is_some());
            assert!(a.get("v").and_then(Value::as_str).is_some());
        }
    }

    #[test]
    fn empty_relay_base_flush_is_noop() {
        // A backend with no destination must drop drained events without panic.
        let backend = OtlpBackend::new(String::new(), SAMPLE_ID.to_string());
        backend.capture(Event::new("e"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("rt");
        rt.block_on(backend.flush());
        // Buffer drained; a second flush is also a clean no-op.
        rt.block_on(backend.flush());
    }
}
