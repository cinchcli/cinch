use super::map;
use crate::crypto;
use crate::http::{HttpError, RestClient};
use crate::store::{queries, Store, StoreError};
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

        // Decrypt before mapping: the HTTP layer returns raw wire bytes and
        // does not decrypt.  Skip clips we cannot decrypt rather than storing
        // ciphertext, which would break FTS5 search and downstream rendering.
        if clip.encrypted {
            match enc_key {
                None => {
                    warn!(
                        clip_id = %clip.clip_id,
                        "backfill: skipping encrypted clip — no encryption key available"
                    );
                    continue;
                }
                Some(key) => {
                    match crypto::decrypt(key, &clip.content) {
                        Ok(plaintext) => {
                            // Store decrypted content as UTF-8.  Non-UTF-8 bytes
                            // (e.g. image data) are preserved via lossy conversion;
                            // callers using the image branch recover bytes from
                            // `StoredClip.content`.
                            clip.content = String::from_utf8_lossy(&plaintext).into_owned();
                            clip.encrypted = false;
                        }
                        Err(e) => {
                            warn!(
                                clip_id = %clip.clip_id,
                                error  = %e,
                                "backfill: skipping encrypted clip — decryption failed"
                            );
                            continue;
                        }
                    }
                }
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
