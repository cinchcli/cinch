//! Decoding the relay's WS frames into [`WsEvent`]s, including the async
//! media-fetch detour for D-routed clips.

use tracing::warn;

use super::{decrypt_clip_content, DecryptFailReason, DecryptOutcome, WsEvent};
use crate::protocol::{
    Clip, WSMessage, ACTION_CLIP_DELETED, ACTION_KEY_EXCHANGE_REQUESTED, ACTION_NEW_CLIP,
    ACTION_PING, ACTION_REVOKED, ACTION_TOKEN_ROTATED,
};
use crate::transport::ClipTransport;

/// Result of a single sync `decode_message` pass. Most actions resolve to a
/// final `Event` immediately; the one exception is a `new_clip` whose
/// ciphertext lives in the media store (D-routing), where the caller must do
/// an async REST fetch before decrypt can run.
#[derive(Debug)]
pub(super) enum DecodeOutcome {
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
pub(super) async fn decode_and_finalize(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

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
