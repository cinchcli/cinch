//! Slim WebSocket subscriber for the relay's `/ws` endpoint.
//!
//! Phase 5 ships this for `cinch pull --watch`. It is intentionally
//! narrower than `desktop/src-tauri/src/ws.rs` (which still owns its
//! tauri-coupled lifecycle, tray status, db inserts, image fetch, and
//! key-exchange responder). Once that desktop logic gets refactored to
//! consume callbacks from this module, the duplicated reconnect/decrypt
//! plumbing inside desktop's ws.rs can shrink to a thin event bridge.
//!
//! The client connects with bearer-token auth via the URL query string
//! (`?token=...`), reads frames, decodes `WSMessage`, decrypts clip
//! content when `encrypted=true`, and forwards every interesting message
//! through an `mpsc::Sender<WsEvent>` provided by the caller.

use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, tungstenite};
use tracing::{debug, info, warn};

use reqwest;
use serde_json;

use crate::crypto;
use crate::protocol::{
    Clip, WSMessage, ACTION_CLIP_DELETED, ACTION_KEY_EXCHANGE_REQUESTED, ACTION_NEW_CLIP,
    ACTION_PING, ACTION_REVOKED, ACTION_TOKEN_ROTATED,
};
use crate::version::ClientInfo;

#[derive(Debug, Clone)]
pub enum WsEvent {
    /// Connection state transitions — emitted on connect, disconnect, retry.
    Status(WsStatus),
    /// New clip received. `plaintext` is the decrypted body for encrypted
    /// clips (already base64-decoded for binary), or the raw `clip.content`
    /// when no encryption key was available or `encrypted=false`.
    NewClip { clip: Box<Clip>, plaintext: Vec<u8> },
    /// Clip deleted on the relay (e.g., retention sweep, manual delete).
    ClipDeleted { clip_id: String },
    /// The caller's device was revoked. Future reconnects will 401.
    Revoked { reason: Option<String> },
    /// Server rotated this device's token. Caller should persist the new
    /// token and reconnect with it.
    TokenRotated {
        token: String,
        device_id: Option<String>,
    },
    /// Another device asked for a key bundle. Desktop handles the ECDH
    /// responder; CLI watchers can ignore.
    KeyExchangeRequested { device_id: Option<String> },
    /// Incoming clip could not be decrypted (missing key or wrong key).
    /// The clip was NOT inserted as plaintext. Callers should surface this
    /// to the user and fire `retry_key_bundle`.
    ClipDecryptFailed {
        clip_id: String,
        reason: DecryptFailReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsStatus {
    Connecting,
    Connected,
    Disconnected,
}

#[derive(Debug, thiserror::Error)]
pub enum WsError {
    #[error("ws: {0}")]
    Tungstenite(#[from] tungstenite::Error),
    #[error("decode: {0}")]
    Decode(String),
}

#[derive(Debug, Clone)]
pub struct WsConfig {
    /// Base relay URL (http/https). The ws module exchanges a short-lived
    /// ticket from POST /ws/ticket before connecting; the bearer token
    /// never appears in the WebSocket URL or server access logs.
    pub relay_url: String,
    /// Bearer token used to obtain a WS ticket from POST /ws/ticket.
    pub token: String,
    /// 32-byte AES key used to decrypt incoming `encrypted=true` clips.
    /// Pass `None` to skip decryption (encrypted clips reach the caller
    /// with `clip.content` set to ciphertext).
    pub encryption_key: Option<[u8; 32]>,
    /// Optional self-identification. When `Some`, the client sends a single
    /// `client_hello` WS frame right after connect (before entering the read
    /// loop) so the relay can record the caller's binary type + version
    /// without a follow-up REST call. `None` preserves the legacy behavior
    /// of staying silent until the first server frame arrives.
    pub client_info: Option<ClientInfo>,
}

/// Outcome of a single decrypt attempt on an incoming clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecryptOutcome {
    /// Clip was not flagged encrypted; nothing to do.
    Plaintext,
    /// Successfully decrypted; `clip.content` + `clip.encrypted` mutated in place.
    Decoded,
    /// No AES key in local store; clip left untouched.
    MissingKey,
    /// AES-GCM tag verification failed (likely key mismatch); clip left untouched.
    TagFailed { error: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecryptFailReason {
    MissingKey,
    TagFailed(String),
}

/// Connect to the relay and forward decoded events to `tx` until the
/// caller drops the receiver. Reconnects on socket error with exponential
/// backoff (1s, 2s, 4s … capped at 30s). Returns when `tx` is closed.
pub async fn run(cfg: WsConfig, tx: mpsc::Sender<WsEvent>) {
    let mut attempt = 0u32;
    loop {
        if tx.is_closed() {
            return;
        }
        let _ = tx.send(WsEvent::Status(WsStatus::Connecting)).await;
        match connect_and_listen(&cfg, &tx).await {
            Ok(()) => {
                debug!("ws: closed cleanly");
                attempt = 0;
            }
            Err(e) => {
                warn!("ws error: {}", e);
                attempt = attempt.saturating_add(1);
            }
        }
        let _ = tx.send(WsEvent::Status(WsStatus::Disconnected)).await;
        let backoff_secs = 1u64 << attempt.min(5); // 1, 2, 4, 8, 16, 32 ...
        sleep(Duration::from_secs(backoff_secs.min(30))).await;
    }
}

/// Fetch a short-lived single-use WebSocket ticket from the relay.
/// Calls POST /ws/ticket with a Bearer auth header; returns the hex ticket string.
async fn fetch_ws_ticket(relay_url: &str, token: &str) -> Result<String, WsError> {
    let ticket_url = format!("{}/ws/ticket", relay_url.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| WsError::Decode(format!("build http client: {}", e)))?;
    let resp = client
        .post(&ticket_url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| WsError::Decode(format!("ticket request: {}", e)))?;
    if !resp.status().is_success() {
        return Err(WsError::Decode(format!(
            "ticket endpoint returned {}",
            resp.status()
        )));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| WsError::Decode(format!("parse ticket response: {}", e)))?;
    body["ticket"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| WsError::Decode("no ticket in response".into()))
}

async fn connect_and_listen(cfg: &WsConfig, tx: &mpsc::Sender<WsEvent>) -> Result<(), WsError> {
    let ticket = fetch_ws_ticket(&cfg.relay_url, &cfg.token).await?;
    let ws_base = cfg
        .relay_url
        .replace("https://", "wss://")
        .replace("http://", "ws://");
    let ws_url = format!("{}/ws?ticket={}", ws_base.trim_end_matches('/'), ticket);
    let (ws_stream, _) = connect_async(&ws_url).await?;
    info!("ws connected");
    log::info!("ws connected"); // also via log crate so env_logger captures it
    let _ = tx.send(WsEvent::Status(WsStatus::Connected)).await;

    let (mut write, mut read) = ws_stream.split();

    // Self-identify before the read loop. The relay's hub dispatches by
    // action — `client_hello` is just one of the actions it expects to see
    // and records into the devices table. On marshal/send failure we let the
    // outer `run` reconnect with backoff; if the payload itself is malformed
    // the warn-level log surfaces it.
    if let Some(info) = cfg.client_info.as_ref() {
        let hello = info.client_hello_message();
        match serde_json::to_string(&hello) {
            Ok(text) => {
                write.send(Message::Text(text.into())).await?;
            }
            Err(e) => {
                warn!("ws: failed to serialize client_hello: {}", e);
            }
        }
    }

    while let Some(frame) = read.next().await {
        let msg = frame?;
        match msg {
            Message::Text(text) => {
                if let Some(event) = decode_message(text.as_str(), cfg.encryption_key) {
                    if tx.send(event).await.is_err() {
                        return Ok(());
                    }
                }
            }
            Message::Ping(data) => {
                write.send(Message::Pong(data)).await?;
            }
            Message::Close(_) => {
                debug!("relay sent close");
                return Ok(());
            }
            _ => {}
        }
    }
    Ok(())
}

fn decode_message(text: &str, key: Option<[u8; 32]>) -> Option<WsEvent> {
    let msg: WSMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            warn!("ws: bad message: {}", e);
            return None;
        }
    };

    match msg.action.as_str() {
        ACTION_NEW_CLIP => {
            let mut clip = msg.clip?;
            match decrypt_clip_content(&mut clip, key) {
                DecryptOutcome::Plaintext => {
                    let plaintext = clip.content.as_bytes().to_vec();
                    Some(WsEvent::NewClip {
                        clip: Box::new(clip),
                        plaintext,
                    })
                }
                DecryptOutcome::Decoded => {
                    let plaintext = clip.content.as_bytes().to_vec();
                    Some(WsEvent::NewClip {
                        clip: Box::new(clip),
                        plaintext,
                    })
                }
                DecryptOutcome::MissingKey => Some(WsEvent::ClipDecryptFailed {
                    clip_id: clip.clip_id,
                    reason: DecryptFailReason::MissingKey,
                }),
                DecryptOutcome::TagFailed { error } => Some(WsEvent::ClipDecryptFailed {
                    clip_id: clip.clip_id,
                    reason: DecryptFailReason::TagFailed(error),
                }),
            }
        }
        ACTION_CLIP_DELETED => Some(WsEvent::ClipDeleted {
            clip_id: msg.clip.map(|c| c.clip_id).unwrap_or_default(),
        }),
        ACTION_REVOKED => Some(WsEvent::Revoked { reason: msg.reason }),
        ACTION_TOKEN_ROTATED => msg.token.map(|t| WsEvent::TokenRotated {
            token: t,
            device_id: msg.device_id,
        }),
        ACTION_KEY_EXCHANGE_REQUESTED => {
            log::info!(
                "ws: decoded key_exchange_requested device_id={:?}",
                msg.device_id
            );
            Some(WsEvent::KeyExchangeRequested {
                device_id: msg.device_id,
            })
        }
        ACTION_PING => None, // server pings handled by tungstenite Pong frames
        _ => None,
    }
}

/// Decrypt `clip.content` in place if `clip.encrypted` and a key is available.
/// Returns a typed outcome — never silently returns ciphertext as plaintext.
pub fn decrypt_clip_content(clip: &mut Clip, key: Option<[u8; 32]>) -> DecryptOutcome {
    if !clip.encrypted {
        return DecryptOutcome::Plaintext;
    }
    let Some(key) = key else {
        return DecryptOutcome::MissingKey;
    };
    let plaintext = match crypto::decrypt(&key, &clip.content) {
        Ok(p) => p,
        Err(e) => {
            return DecryptOutcome::TagFailed {
                error: e.to_string(),
            }
        }
    };
    let is_binary = clip
        .media_path
        .as_deref()
        .filter(|p| !p.is_empty())
        .is_some()
        || clip.content_type.starts_with("image");
    if is_binary {
        // Re-encode as base64 so the struct stays a valid String.
        clip.content = STANDARD.encode(&plaintext);
    } else {
        match String::from_utf8(plaintext) {
            Ok(s) => clip.content = s,
            Err(e) => {
                return DecryptOutcome::TagFailed {
                    error: format!("post-decrypt utf-8 invalid: {e}"),
                }
            }
        }
    }
    clip.encrypted = false;
    DecryptOutcome::Decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(action: &str, body: serde_json::Value) -> String {
        let mut v = body;
        v.as_object_mut()
            .unwrap()
            .insert("action".into(), serde_json::Value::String(action.into()));
        serde_json::to_string(&v).unwrap()
    }

    #[test]
    fn decodes_new_clip_unencrypted() {
        let json = make_msg(
            ACTION_NEW_CLIP,
            serde_json::json!({
                "clip": {
                    "clip_id": "01H",
                    "user_id": "u1",
                    "content": "hello",
                    "content_type": "text",
                    "source": "remote:host",
                    "created_at": "2026-04-30T00:00:00Z",
                    "encrypted": false
                }
            }),
        );
        match decode_message(&json, None).unwrap() {
            WsEvent::NewClip { clip, plaintext } => {
                assert_eq!(clip.clip_id, "01H");
                assert_eq!(plaintext, b"hello");
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn decodes_revoked() {
        let json = make_msg(
            ACTION_REVOKED,
            serde_json::json!({"reason": "device removed"}),
        );
        match decode_message(&json, None).unwrap() {
            WsEvent::Revoked { reason } => assert_eq!(reason.as_deref(), Some("device removed")),
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn decodes_clip_deleted() {
        let json = make_msg(
            ACTION_CLIP_DELETED,
            serde_json::json!({
                "clip": {
                    "clip_id": "delme",
                    "user_id": "u1",
                    "content": "",
                    "content_type": "text",
                    "source": "local",
                    "created_at": "2026-04-30T00:00:00Z"
                }
            }),
        );
        match decode_message(&json, None).unwrap() {
            WsEvent::ClipDeleted { clip_id } => assert_eq!(clip_id, "delme"),
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn decrypts_text_clip_with_key() {
        let key = [0x42u8; 32];
        let ciphertext = crypto::encrypt(&key, b"secret payload").unwrap();
        let json = make_msg(
            ACTION_NEW_CLIP,
            serde_json::json!({
                "clip": {
                    "clip_id": "01H",
                    "user_id": "u1",
                    "content": ciphertext,
                    "content_type": "text",
                    "source": "remote:host",
                    "created_at": "2026-04-30T00:00:00Z",
                    "encrypted": true
                }
            }),
        );
        match decode_message(&json, Some(key)).unwrap() {
            WsEvent::NewClip { clip, plaintext } => {
                assert_eq!(plaintext, b"secret payload");
                assert!(!clip.encrypted);
                assert_eq!(clip.content, "secret payload");
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }

    #[test]
    fn decrypt_failure_does_not_silently_return_ciphertext() {
        let sender_key = [0x11u8; 32];
        let receiver_key = [0x22u8; 32];
        let blob = crypto::encrypt(&sender_key, b"hello from remote cli").unwrap();

        let mut clip = Clip {
            clip_id: "c1".into(),
            user_id: "u1".into(),
            content: blob.clone(),
            content_type: String::new(),
            encrypted: true,
            ..Default::default()
        };

        let outcome = decrypt_clip_content(&mut clip, Some(receiver_key));

        assert!(
            matches!(outcome, DecryptOutcome::TagFailed { .. }),
            "wrong-key decrypt must return TagFailed, got {:?}",
            outcome
        );
        assert!(clip.encrypted, "encrypted flag must remain true on failure");
        assert_eq!(
            clip.content, blob,
            "content must not be replaced with garbage plaintext"
        );
    }

    #[test]
    fn decrypt_missing_key_returns_missing_key_outcome() {
        let sender_key = [0x33u8; 32];
        let blob = crypto::encrypt(&sender_key, b"secret").unwrap();

        let mut clip = Clip {
            clip_id: "c2".into(),
            user_id: "u1".into(),
            content: blob.clone(),
            content_type: String::new(),
            encrypted: true,
            ..Default::default()
        };

        let outcome = decrypt_clip_content(&mut clip, None);
        assert_eq!(outcome, DecryptOutcome::MissingKey);
        assert!(
            clip.encrypted,
            "clip must remain encrypted when key is missing"
        );
        assert_eq!(
            clip.content, blob,
            "content must be untouched when key is missing"
        );
    }

    #[test]
    fn wrong_key_via_decode_message_emits_clip_decrypt_failed() {
        let sender_key = [0x44u8; 32];
        let receiver_key = [0x55u8; 32];
        let blob = crypto::encrypt(&sender_key, b"payload").unwrap();

        let json = make_msg(
            ACTION_NEW_CLIP,
            serde_json::json!({
                "clip": {
                    "clip_id": "bad-clip",
                    "user_id": "u1",
                    "content": blob,
                    "content_type": "text",
                    "source": "remote:host",
                    "created_at": "2026-04-30T00:00:00Z",
                    "encrypted": true
                }
            }),
        );
        match decode_message(&json, Some(receiver_key)).unwrap() {
            WsEvent::ClipDecryptFailed { clip_id, reason } => {
                assert_eq!(clip_id, "bad-clip");
                assert!(matches!(reason, DecryptFailReason::TagFailed(_)));
            }
            other => panic!("expected ClipDecryptFailed, got {:?}", other),
        }
    }

    #[test]
    fn missing_key_via_decode_message_emits_clip_decrypt_failed() {
        let sender_key = [0x66u8; 32];
        let blob = crypto::encrypt(&sender_key, b"payload").unwrap();

        let json = make_msg(
            ACTION_NEW_CLIP,
            serde_json::json!({
                "clip": {
                    "clip_id": "no-key-clip",
                    "user_id": "u1",
                    "content": blob,
                    "content_type": "text",
                    "source": "remote:host",
                    "created_at": "2026-04-30T00:00:00Z",
                    "encrypted": true
                }
            }),
        );
        match decode_message(&json, None).unwrap() {
            WsEvent::ClipDecryptFailed { clip_id, reason } => {
                assert_eq!(clip_id, "no-key-clip");
                assert_eq!(reason, DecryptFailReason::MissingKey);
            }
            other => panic!("expected ClipDecryptFailed, got {:?}", other),
        }
    }
}
