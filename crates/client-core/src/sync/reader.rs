use super::map;
use crate::http::{HttpError, RestClient};
use crate::store::{queries, Store, StoreError};
use crate::ws::{decrypt_clip_content, needs_media_fetch, DecryptOutcome};
use std::time::{Duration, Instant};
use tracing::warn;

/// Wall-time and volume budget for a single backfill pass.
pub struct BackfillBudget {
    /// Maximum elapsed time before the pass is cut short.
    pub max_wall: Duration,
    /// Maximum number of clips to fetch from the relay in one pass.
    pub max_clips: u32,
}

impl Default for BackfillBudget {
    fn default() -> Self {
        Self {
            max_wall: Duration::from_secs(2),
            max_clips: 500,
        }
    }
}

/// Convert a stored ULID watermark string to a `chrono::DateTime<chrono::Utc>`
/// so it can be passed to [`RestClient::list_clips_since`].
///
/// Returns `None` if the string is empty or cannot be parsed as a ULID.
fn ulid_to_datetime(ulid_str: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    use std::time::UNIX_EPOCH;
    let ulid = ulid::Ulid::from_string(ulid_str).ok()?;
    let ms = ulid.datetime().duration_since(UNIX_EPOCH).ok()?.as_millis() as i64;
    chrono::DateTime::from_timestamp_millis(ms)
}

/// One pass of REST backfill from `meta.last_sync_watermark` forward.
///
/// Fetches clips newer than the stored watermark, decrypts any encrypted clips
/// using `enc_key` (when provided), writes them into the local store, and
/// advances the watermark to the highest clip ID seen.  Returns the number of
/// clips inserted.
///
/// The HTTP layer does **not** decrypt — callers must supply the AES-256 key
/// when the account uses client-side encryption.  If an encrypted clip is
/// encountered and `enc_key` is `None`, or decryption fails, the clip is
/// logged and skipped (same policy as design doc §9 risk item).
pub async fn backfill_once(
    store: &Store,
    client: &RestClient,
    budget: BackfillBudget,
    enc_key: Option<&[u8; 32]>,
) -> Result<usize, BackfillError> {
    let start = Instant::now();

    // Resolve the watermark: stored as a ULID → extract the embedded timestamp
    // and pass it as the `since` parameter so the relay returns only newer clips.
    let since = queries::watermark(store)
        .map_err(BackfillError::Store)?
        .and_then(|w| ulid_to_datetime(&w));

    let clips = client
        .list_clips_since(since, budget.max_clips)
        .await
        .map_err(BackfillError::Http)?;

    let mut inserted = 0usize;
    let mut max_id: Option<String> = None;

    for mut clip in clips {
        if start.elapsed() >= budget.max_wall {
            break;
        }

        // Media-routed clips (D-routing) arrive with empty `content` + a
        // `media_path` pointer. Fetch the ciphertext bytes from
        // /clips/{id}/media so decrypt_clip_content has something to chew on.
        if needs_media_fetch(&clip) {
            match client.get_clip_media(&clip.clip_id).await {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(s) => clip.content = s,
                    Err(e) => {
                        warn!(
                            clip_id = %clip.clip_id,
                            error = %e,
                            "backfill: media bytes not utf-8 — skipping clip"
                        );
                        continue;
                    }
                },
                Err(e) => {
                    warn!(
                        clip_id = %clip.clip_id,
                        error = %e,
                        "backfill: media fetch failed — skipping clip"
                    );
                    continue;
                }
            }
        }

        // Decrypt via the shared helper. For binary clips (image/* or
        // media_path-routed) this re-encodes the plaintext to base64 so
        // `clip_wire_to_stored` (which expects wire base64 for binary) writes
        // raw bytes to the local store. Non-image text clips are decoded to
        // UTF-8 strictly — invalid UTF-8 surfaces as TagFailed and is skipped
        // rather than lossily replaced with U+FFFD.
        match decrypt_clip_content(&mut clip, enc_key.copied()) {
            DecryptOutcome::Plaintext | DecryptOutcome::Decoded => {}
            DecryptOutcome::MissingKey => {
                warn!(
                    clip_id = %clip.clip_id,
                    "backfill: skipping encrypted clip — no encryption key available"
                );
                continue;
            }
            DecryptOutcome::TagFailed { error } => {
                warn!(
                    clip_id = %clip.clip_id,
                    error = %error,
                    "backfill: skipping encrypted clip — decryption failed"
                );
                continue;
            }
        }

        let stored = match map::clip_wire_to_stored(&clip).map_err(BackfillError::Map)? {
            Some(c) => c,
            None => continue,
        };
        // Track the lexicographically largest ULID to use as the new watermark.
        // ULIDs sort lexicographically in time order, so the max string ID is
        // the most recent clip.
        if max_id
            .as_deref()
            .map(|m| stored.id.as_str() > m)
            .unwrap_or(true)
        {
            max_id = Some(stored.id.clone());
        }
        queries::insert_clip(store, &stored).map_err(BackfillError::Store)?;
        inserted += 1;
    }

    if let Some(id) = max_id {
        queries::set_watermark(store, &id).map_err(BackfillError::Store)?;
    }

    Ok(inserted)
}

/// Errors that can occur during a backfill pass.
#[derive(Debug, thiserror::Error)]
pub enum BackfillError {
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("http: {0}")]
    Http(HttpError),
    #[error("map: {0}")]
    Map(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto;
    use crate::version::{ClientInfo, ClientType};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn rest(uri: String) -> RestClient {
        RestClient::new(
            uri,
            "tok",
            ClientInfo {
                client_type: ClientType::Cli,
                version: "0".into(),
            },
        )
        .expect("RestClient")
    }

    #[tokio::test]
    async fn backfill_fetches_media_routed_image_and_stores_raw_bytes() {
        // Mirror the WS path test: relay returns an image clip with empty
        // content + media_path. Backfill must GET /clips/{id}/media, run
        // decrypt_clip_content (image branch → base64 re-encode), then let
        // clip_wire_to_stored base64-decode back to raw PNG bytes.
        let server = MockServer::start().await;
        let key = [0xaau8; 32];
        let png = vec![0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let ciphertext = crypto::encrypt(&key, &png).unwrap();
        let clip_id = "01JABCDEFGHJKMNPQRSTVWXYZ0";

        Mock::given(method("GET"))
            .and(path("/clips"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "clip_id": clip_id,
                    "user_id": "u",
                    "content": "",
                    "content_type": "image",
                    "source": "remote:cli",
                    "created_at": "2026-04-30T00:00:00Z",
                    "encrypted": true,
                    "media_path": "clips/abc.bin",
                }
            ])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/clips/{}/media", clip_id)))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(ciphertext.into_bytes(), "application/octet-stream"),
            )
            .mount(&server)
            .await;

        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let inserted = backfill_once(
            &store,
            &rest(server.uri()),
            BackfillBudget::default(),
            Some(&key),
        )
        .await
        .expect("backfill_once");
        assert_eq!(inserted, 1);

        let stored = queries::get_clip(&store, clip_id)
            .unwrap()
            .expect("clip must be stored");
        assert_eq!(stored.content.as_deref(), Some(&png[..]));
        assert_eq!(stored.content_type, "image");
    }

    #[tokio::test]
    async fn backfill_skips_media_clip_when_fetch_fails() {
        // Media fetch errors must not poison the rest of the backfill batch
        // — the bad clip is logged and skipped while subsequent rows still
        // land. Here only one clip is returned, so we just verify the count.
        let server = MockServer::start().await;
        let clip_id = "01JABCDEFGHJKMNPQRSTVWXYZ0";

        Mock::given(method("GET"))
            .and(path("/clips"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "clip_id": clip_id,
                    "user_id": "u",
                    "content": "",
                    "content_type": "image",
                    "source": "remote:cli",
                    "created_at": "2026-04-30T00:00:00Z",
                    "encrypted": true,
                    "media_path": "clips/abc.bin",
                }
            ])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/clips/{}/media", clip_id)))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let inserted = backfill_once(
            &store,
            &rest(server.uri()),
            BackfillBudget::default(),
            Some(&[0u8; 32]),
        )
        .await
        .expect("backfill_once");
        assert_eq!(inserted, 0);
        assert!(queries::get_clip(&store, clip_id).unwrap().is_none());
    }
}
