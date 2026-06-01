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
use crate::transport::ClipTransport;
use crate::version::ClientInfo;
use std::sync::Arc;

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
    /// REST client used to fetch ciphertext for clips that arrive with
    /// `media_path` set + empty `content` (the relay D-routing path). When
    /// `None`, such clips surface as `ClipDecryptFailed` so the caller can
    /// see the misconfiguration instead of silently dropping the row.
    pub media_fetcher: Option<Arc<dyn ClipTransport>>,
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
///
/// `pub(crate)` on purpose: the sole production entry point is
/// [`crate::transport::WsStreamTransport`], which both the sync
/// [`Writer`](crate::sync::Writer) and the CLI `pull --watch` loop drive
/// through the [`StreamTransport`](crate::transport::StreamTransport) seam.
/// Routing every caller through that seam is what keeps the streaming side
/// mockable; calling `run` directly would bypass it.
pub(crate) async fn run(cfg: WsConfig, tx: mpsc::Sender<WsEvent>) {
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
                if let Some(event) = decode_and_finalize(
                    text.as_str(),
                    cfg.encryption_key,
                    cfg.media_fetcher.as_deref(),
                )
                .await
                {
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

/// Result of a single sync `decode_message` pass. Most actions resolve to a
/// final `Event` immediately; the one exception is a `new_clip` whose
/// ciphertext lives in the media store (D-routing), where the caller must do
/// an async REST fetch before decrypt can run.
#[derive(Debug)]
enum DecodeOutcome {
    Event(WsEvent),
    NeedsMediaFetch(Box<Clip>),
}

/// True when the wire frame carries an empty `content` + a non-empty
/// `media_path` — i.e. the relay routed the encrypted bytes through the
/// media store and stripped the inline copy. Caller must fetch from
/// `/clips/{id}/media`. Shared between the WS receive path and the HTTP
/// backfill path so both make the same decision.
pub(crate) fn needs_media_fetch(clip: &Clip) -> bool {
    clip.content.is_empty()
        && clip
            .media_path
            .as_deref()
            .filter(|p| !p.is_empty())
            .is_some()
}

/// Run decrypt + map a `Clip` whose ciphertext is already in `content` into
/// the appropriate `WsEvent`. Shared between the inline-content path and the
/// media-fetch path so both produce identical `NewClip` envelopes.
fn finalize_new_clip(mut clip: Clip, key: Option<[u8; 32]>) -> WsEvent {
    match decrypt_clip_content(&mut clip, key) {
        DecryptOutcome::Plaintext | DecryptOutcome::Decoded => {
            let plaintext = clip.content.as_bytes().to_vec();
            WsEvent::NewClip {
                clip: Box::new(clip),
                plaintext,
            }
        }
        DecryptOutcome::MissingKey => WsEvent::ClipDecryptFailed {
            clip_id: clip.clip_id,
            reason: DecryptFailReason::MissingKey,
        },
        DecryptOutcome::TagFailed { error } => WsEvent::ClipDecryptFailed {
            clip_id: clip.clip_id,
            reason: DecryptFailReason::TagFailed(error),
        },
    }
}

fn decode_message(text: &str, key: Option<[u8; 32]>) -> Option<DecodeOutcome> {
    let msg: WSMessage = match serde_json::from_str(text) {
        Ok(m) => m,
        Err(e) => {
            warn!("ws: bad message: {}", e);
            return None;
        }
    };

    match msg.action.as_str() {
        ACTION_NEW_CLIP => {
            let clip = msg.clip?;
            if needs_media_fetch(&clip) {
                return Some(DecodeOutcome::NeedsMediaFetch(Box::new(clip)));
            }
            Some(DecodeOutcome::Event(finalize_new_clip(clip, key)))
        }
        ACTION_CLIP_DELETED => Some(DecodeOutcome::Event(WsEvent::ClipDeleted {
            clip_id: msg.clip.map(|c| c.clip_id).unwrap_or_default(),
        })),
        ACTION_REVOKED => Some(DecodeOutcome::Event(WsEvent::Revoked {
            reason: msg.reason,
        })),
        ACTION_TOKEN_ROTATED => msg.token.map(|t| {
            DecodeOutcome::Event(WsEvent::TokenRotated {
                token: t,
                device_id: msg.device_id,
            })
        }),
        ACTION_KEY_EXCHANGE_REQUESTED => {
            log::info!(
                "ws: decoded key_exchange_requested device_id={:?}",
                msg.device_id
            );
            Some(DecodeOutcome::Event(WsEvent::KeyExchangeRequested {
                device_id: msg.device_id,
            }))
        }
        ACTION_PING => None, // server pings handled by tungstenite Pong frames
        _ => None,
    }
}

/// Async wrapper: parses the WS frame, fetches media if needed, and returns
/// a final `WsEvent`. Returns `None` for frames that should be silently
/// dropped (ping, malformed, unknown actions). Failures during media fetch
/// surface as `ClipDecryptFailed` so the caller can surface a UI signal.
async fn decode_and_finalize(
    text: &str,
    key: Option<[u8; 32]>,
    media_fetcher: Option<&dyn ClipTransport>,
) -> Option<WsEvent> {
    let outcome = decode_message(text, key)?;
    match outcome {
        DecodeOutcome::Event(e) => Some(e),
        DecodeOutcome::NeedsMediaFetch(clip) => {
            let clip_id = clip.clip_id.clone();
            let Some(fetcher) = media_fetcher else {
                warn!(
                    "ws: dropping media-routed clip {} — no http client configured",
                    clip_id
                );
                return Some(WsEvent::ClipDecryptFailed {
                    clip_id,
                    reason: DecryptFailReason::TagFailed(
                        "media fetch unavailable (no http client)".into(),
                    ),
                });
            };
            match fetcher.get_clip_media(&clip_id).await {
                Ok(bytes) => {
                    let mut c = *clip;
                    // The relay stores the ciphertext as the raw bytes of
                    // the wire `Clip.content` String (which itself is the
                    // base64 ciphertext produced by crypto::encrypt). Those
                    // bytes are ASCII; `from_utf8` is total here.
                    c.content = match String::from_utf8(bytes) {
                        Ok(s) => s,
                        Err(e) => {
                            warn!("ws: media bytes for {} not valid utf-8: {}", clip_id, e);
                            return Some(WsEvent::ClipDecryptFailed {
                                clip_id,
                                reason: DecryptFailReason::TagFailed(format!(
                                    "media bytes not utf-8: {e}"
                                )),
                            });
                        }
                    };
                    Some(finalize_new_clip(c, key))
                }
                Err(e) => {
                    warn!("ws: media fetch failed for {}: {}", clip_id, e);
                    Some(WsEvent::ClipDecryptFailed {
                        clip_id,
                        reason: DecryptFailReason::TagFailed(format!("media fetch: {e}")),
                    })
                }
            }
        }
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
            DecodeOutcome::Event(WsEvent::NewClip { clip, plaintext }) => {
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
            DecodeOutcome::Event(WsEvent::Revoked { reason }) => {
                assert_eq!(reason.as_deref(), Some("device removed"))
            }
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
            DecodeOutcome::Event(WsEvent::ClipDeleted { clip_id }) => {
                assert_eq!(clip_id, "delme")
            }
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
            DecodeOutcome::Event(WsEvent::NewClip { clip, plaintext }) => {
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
            DecodeOutcome::Event(WsEvent::ClipDecryptFailed { clip_id, reason }) => {
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
            DecodeOutcome::Event(WsEvent::ClipDecryptFailed { clip_id, reason }) => {
                assert_eq!(clip_id, "no-key-clip");
                assert_eq!(reason, DecryptFailReason::MissingKey);
            }
            other => panic!("expected ClipDecryptFailed, got {:?}", other),
        }
    }

    #[test]
    fn media_routed_clip_decodes_as_needs_media_fetch() {
        // Relay D2b broadcasts media-routed image clips with empty content +
        // a media_path pointer. Sync decode_message must surface that as
        // NeedsMediaFetch so the async caller knows to GET /clips/{id}/media
        // before attempting decrypt.
        let json = make_msg(
            ACTION_NEW_CLIP,
            serde_json::json!({
                "clip": {
                    "clip_id": "img1",
                    "user_id": "u1",
                    "content": "",
                    "content_type": "image",
                    "source": "remote:cli",
                    "created_at": "2026-04-30T00:00:00Z",
                    "encrypted": true,
                    "media_path": "clips/01J.bin"
                }
            }),
        );
        match decode_message(&json, Some([0u8; 32])).unwrap() {
            DecodeOutcome::NeedsMediaFetch(clip) => {
                assert_eq!(clip.clip_id, "img1");
                assert_eq!(clip.media_path.as_deref(), Some("clips/01J.bin"));
                assert!(clip.content.is_empty());
            }
            other => panic!("expected NeedsMediaFetch, got {:?}", other),
        }
    }

    #[test]
    fn dual_write_image_decodes_inline_without_media_fetch() {
        // D1 dual-write: relay populates BOTH content + media_path for image
        // clips. `decode_message` must take the inline path (Event) instead
        // of NeedsMediaFetch — fetching when the ciphertext is already in
        // hand would be wasted bandwidth.
        let key = [0x77u8; 32];
        let plaintext = b"image-bytes-pretend".to_vec();
        let ciphertext = crypto::encrypt(&key, &plaintext).unwrap();
        let json = make_msg(
            ACTION_NEW_CLIP,
            serde_json::json!({
                "clip": {
                    "clip_id": "dual1",
                    "user_id": "u1",
                    "content": ciphertext,
                    "content_type": "image",
                    "source": "remote:cli",
                    "created_at": "2026-04-30T00:00:00Z",
                    "encrypted": true,
                    "media_path": "clips/01J.bin"
                }
            }),
        );
        match decode_message(&json, Some(key)).unwrap() {
            DecodeOutcome::Event(WsEvent::NewClip { clip, .. }) => {
                assert!(!clip.encrypted);
                // decrypt_clip_content re-encodes binary plaintext to base64
                // so the wire `Clip.content: String` invariant holds.
                let decoded = STANDARD.decode(&clip.content).expect("base64");
                assert_eq!(decoded, plaintext);
            }
            other => panic!("expected NewClip from inline content, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn decode_and_finalize_without_fetcher_yields_decrypt_failed() {
        // Safety net: if a relay is configured to media-route but the client
        // forgot to pass a media_fetcher, the resulting drop should be loud
        // (ClipDecryptFailed) rather than silent.
        let json = make_msg(
            ACTION_NEW_CLIP,
            serde_json::json!({
                "clip": {
                    "clip_id": "no-fetcher",
                    "user_id": "u1",
                    "content": "",
                    "content_type": "image",
                    "source": "remote:cli",
                    "created_at": "2026-04-30T00:00:00Z",
                    "encrypted": true,
                    "media_path": "clips/x.bin"
                }
            }),
        );
        match decode_and_finalize(&json, Some([0u8; 32]), None).await {
            Some(WsEvent::ClipDecryptFailed { clip_id, reason }) => {
                assert_eq!(clip_id, "no-fetcher");
                assert!(matches!(reason, DecryptFailReason::TagFailed(_)));
            }
            other => panic!("expected ClipDecryptFailed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn decode_and_finalize_fetches_media_and_decrypts() {
        // End-to-end happy path: WS frame says media_path. Client fetches
        // ciphertext bytes from /clips/{id}/media via a stub HTTP server,
        // then runs decrypt_clip_content with the AES key.
        use crate::version::{ClientInfo, ClientType};
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let key = [0x9au8; 32];
        let plaintext = b"image-bytes-pretend".to_vec();
        let ciphertext = crypto::encrypt(&key, &plaintext).unwrap();

        Mock::given(method("GET"))
            .and(path("/clips/img-fetch/media"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(ciphertext.clone().into_bytes(), "application/octet-stream"),
            )
            .mount(&server)
            .await;

        let rest = crate::http::RestClient::new(
            server.uri(),
            "tok",
            ClientInfo {
                client_type: ClientType::Cli,
                version: "0".into(),
            },
        )
        .expect("RestClient");

        let json = make_msg(
            ACTION_NEW_CLIP,
            serde_json::json!({
                "clip": {
                    "clip_id": "img-fetch",
                    "user_id": "u1",
                    "content": "",
                    "content_type": "image",
                    "source": "remote:cli",
                    "created_at": "2026-04-30T00:00:00Z",
                    "encrypted": true,
                    "media_path": "clips/abc.bin"
                }
            }),
        );

        match decode_and_finalize(&json, Some(key), Some(&rest)).await {
            Some(WsEvent::NewClip {
                clip,
                plaintext: pt,
            }) => {
                assert_eq!(clip.clip_id, "img-fetch");
                assert!(!clip.encrypted);
                // For image clips, decrypt_clip_content stores plaintext as
                // base64 in clip.content; the event's `plaintext` field is
                // those base64 bytes (sync::map::clip_wire_to_stored later
                // decodes them when persisting to the local SQLite store).
                assert_eq!(pt, clip.content.as_bytes().to_vec());
                let decoded = STANDARD.decode(&clip.content).expect("base64");
                assert_eq!(decoded, plaintext);
            }
            other => panic!("expected NewClip after media fetch, got {:?}", other),
        }
    }
}
