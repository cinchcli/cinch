use std::sync::Arc;

use tauri::State;

use super::{
    image_bytes_for, resolve_active_creds, source_row_to_info, stored_to_local, LocalClip,
    SourceInfo,
};
use crate::clipboard::ClipboardService;
use crate::protocol::MultiConfigHandle;
use crate::store::db::Database;
use crate::SharedStore;
use client_core::store::queries;

// ---------------------------------------------------------------------------
// Pinning commands — delegated to client_core::store::queries
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn list_pinned_clips(store: State<'_, SharedStore>) -> Result<Vec<LocalClip>, String> {
    queries::list_clips(&store, None, None, None, true, 200)
        .map(|v| v.into_iter().map(stored_to_local).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn pin_clip(
    store: State<'_, SharedStore>,
    mc: State<'_, MultiConfigHandle>,
    id: String,
    note: Option<String>,
) -> Result<(), String> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    queries::set_pinned(&store, &id, true, now_ms).map_err(|e| e.to_string())?;
    if let Ok((relay_url, token)) = resolve_active_creds(&mc) {
        match client_core::http::RestClient::new(relay_url, token, crate::build_client_info()) {
            Ok(client) => {
                if let Err(e) = client.set_clip_pin(&id, true, note.as_deref()).await {
                    log::warn!("relay set_clip_pin failed for {}: {}", id, e);
                }
            }
            Err(e) => {
                log::warn!("could not build REST client for pin_clip {}: {}", id, e);
            }
        }
    }
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub async fn unpin_clip(
    store: State<'_, SharedStore>,
    mc: State<'_, MultiConfigHandle>,
    id: String,
) -> Result<(), String> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    queries::set_pinned(&store, &id, false, now_ms).map_err(|e| e.to_string())?;
    if let Ok((relay_url, token)) = resolve_active_creds(&mc) {
        match client_core::http::RestClient::new(relay_url, token, crate::build_client_info()) {
            Ok(client) => {
                if let Err(e) = client.set_clip_pin(&id, false, None).await {
                    log::warn!("relay unpin_clip failed for {}: {}", id, e);
                }
            }
            Err(e) => {
                log::warn!("could not build REST client for unpin_clip {}: {}", id, e);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Clip read commands — delegated to client_core::store::queries
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn list_clips(
    store: State<'_, SharedStore>,
    source: Option<String>,
    content_type: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<LocalClip>, String> {
    let clips = queries::list_clips(&store, source.as_deref(), limit, None, false, 50)
        .map_err(|e| e.to_string())?;

    // Optional client-side content_type filter (client-core query has no content_type filter yet).
    let filtered: Vec<LocalClip> = clips
        .into_iter()
        .map(stored_to_local)
        .filter(|c| {
            content_type
                .as_deref()
                .map(|ct| c.content_type == ct)
                .unwrap_or(true)
        })
        .collect();
    Ok(filtered)
}

#[tauri::command]
#[specta::specta]
pub fn search_clips(
    store: State<'_, SharedStore>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<LocalClip>, String> {
    queries::search_clips(&store, &query, limit.unwrap_or(50))
        .map(|v| v.into_iter().map(stored_to_local).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_sources(store: State<'_, SharedStore>) -> Result<Vec<SourceInfo>, String> {
    queries::list_sources(&store)
        .map(|v| v.into_iter().map(source_row_to_info).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn delete_clip(
    store: State<'_, SharedStore>,
    mc: State<'_, MultiConfigHandle>,
    id: String,
) -> Result<(), String> {
    // Best-effort relay deletion: propagate to other devices via clip_deleted broadcast.
    // If the relay is unreachable, log and continue — relay TTL will eventually expire the clip.
    if let Ok((relay_url, token)) = resolve_active_creds(&mc) {
        match client_core::http::RestClient::new(relay_url, token, crate::build_client_info()) {
            Ok(client) => {
                if let Err(e) = client.delete_clip(&id).await {
                    log::warn!("relay delete_clip failed for {}: {}", id, e);
                }
            }
            Err(e) => {
                log::warn!("could not build REST client for delete_clip {}: {}", id, e);
            }
        }
    }
    queries::delete_clip(&store, &id).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_clip_count(store: State<'_, SharedStore>) -> Result<i64, String> {
    queries::clip_count(&store).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// mark_clip_copied — TODO(phase 5): client-core has no copied_at column yet.
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn mark_clip_copied(db: State<'_, Arc<Database>>, id: String) -> Result<(), String> {
    db.mark_clip_copied(&id, chrono::Utc::now().timestamp())
}

// ---------------------------------------------------------------------------
// Clipboard write commands — no store dependency
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn copy_clip_to_clipboard(
    clipboard: State<'_, Arc<ClipboardService>>,
    content: String,
) -> Result<(), String> {
    clipboard.write_text(&content).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn copy_image_to_clipboard(
    clipboard: State<'_, Arc<ClipboardService>>,
    store: State<'_, crate::SharedStore>,
    clip_id: String,
) -> Result<(), String> {
    let bytes = image_bytes_for(store.inner(), &clip_id)?;
    clipboard
        .write_image_png_bytes(&bytes)
        .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Sniff a few well-known image-format magic bytes. Defaults to "png" for
/// unknown payloads: the desktop push pipeline normalises to PNG, so this
/// is the safest fallback for clipboard images that lack a clearer signal.
fn detect_image_ext(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "jpg"
    } else if bytes.starts_with(b"GIF8") {
        "gif"
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "webp"
    } else {
        "png"
    }
}

/// Build the default filename suggested in the save dialog. User can edit it.
/// Shape: `cinch-YYYYMMDD-HHMMSS.<ext>`, timestamp from clip `created_at`
/// (Unix seconds) formatted in the user's local timezone.
fn default_image_filename(created_at_secs: i64, ext: &str) -> String {
    use chrono::TimeZone as _;
    let dt = chrono::Local
        .timestamp_opt(created_at_secs, 0)
        .single()
        .unwrap_or_else(chrono::Local::now);
    format!("cinch-{}.{}", dt.format("%Y%m%d-%H%M%S"), ext)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod helper_tests {
    use super::*;

    #[test]
    fn detect_image_ext_matches_png_magic() {
        let png = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0x00];
        assert_eq!(detect_image_ext(&png), "png");
    }

    #[test]
    fn detect_image_ext_matches_jpeg_magic() {
        let jpg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10];
        assert_eq!(detect_image_ext(&jpg), "jpg");
    }

    #[test]
    fn detect_image_ext_matches_gif_magic() {
        let gif = *b"GIF89a\0\0\0";
        assert_eq!(detect_image_ext(&gif), "gif");
    }

    #[test]
    fn detect_image_ext_matches_webp_magic() {
        let mut webp = Vec::new();
        webp.extend_from_slice(b"RIFF");
        webp.extend_from_slice(&[0x24, 0x00, 0x00, 0x00]); // length placeholder
        webp.extend_from_slice(b"WEBPVP8 ");
        assert_eq!(detect_image_ext(&webp), "webp");
    }

    #[test]
    fn detect_image_ext_defaults_to_png_on_unknown() {
        assert_eq!(detect_image_ext(b""), "png");
        assert_eq!(detect_image_ext(b"not-an-image"), "png");
    }

    #[test]
    fn default_image_filename_uses_created_at_and_ext() {
        // 2026-05-23 15:30:45 UTC — we format in local TZ, so just assert the
        // shape: cinch-YYYYMMDD-HHMMSS.ext
        let name = default_image_filename(1_779_500_000, "png");
        assert!(
            name.starts_with("cinch-") && name.ends_with(".png"),
            "got {name}"
        );
        // 8-digit date, dash, 6-digit time
        assert_eq!(name.len(), "cinch-YYYYMMDD-HHMMSS.png".len());
    }
}
