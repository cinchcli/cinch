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
//!
//! This file owns the connection layer (ticket fetch + reconnect loop) and
//! the wire types. Frame decoding lives in [`decode`], and in-place clip
//! decryption in [`decrypt`].

use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, tungstenite};
use tracing::{debug, info, warn};

use crate::protocol::{Clip, WSMessage};
use crate::transport::ClipTransport;
use crate::version::ClientInfo;

mod decode;
mod decrypt;

pub(crate) use decode::needs_media_fetch;
pub use decrypt::decrypt_clip_content;

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
                match decode::decode_and_finalize(
                    text.as_str(),
                    cfg.encryption_key,
                    cfg.media_fetcher.as_deref(),
                )
                .await
                {
                    // A closed channel means the receiver was dropped, so stop
                    // the read loop cleanly.
                    Some(decode::Frame::Event(event)) => match tx.send(event).await {
                        Ok(()) => {}
                        Err(_) => return Ok(()),
                    },
                    Some(decode::Frame::Pong) => {
                        // Reply to the relay's app-level heartbeat ping. The
                        // relay's keepalive is an app-level {"action":"ping"}
                        // frame, not a protocol Ping, so it needs an explicit
                        // app-level pong. This inbound frame refreshes the
                        // relay's read deadline and keeps this connection in the
                        // hub — without it an idle bearer is reaped and a
                        // `cinch auth retry-key` broadcast reaches nobody.
                        match serde_json::to_string(&WSMessage::pong()) {
                            Ok(t) => write.send(Message::Text(t.into())).await?,
                            Err(e) => warn!("ws: failed to serialize pong: {}", e),
                        }
                    }
                    None => {}
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
