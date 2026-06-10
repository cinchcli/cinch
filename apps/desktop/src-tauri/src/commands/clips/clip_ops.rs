use std::sync::Arc;

use tauri::State;
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;

use super::{
    image_bytes_for, resolve_active_creds, source_row_to_info, stored_to_local, LocalClip,
    SourceInfo,
};
use crate::clipboard::ClipboardService;
use crate::protocol::MultiConfigHandle;
use crate::LocalPusherHandle;
use crate::SharedStore;
use client_core::store::queries;

// ---------------------------------------------------------------------------
// Pinning commands — delegated to client_core::store::queries
// ---------------------------------------------------------------------------

#[tauri::command]
#[specta::specta]
pub fn list_pinned_clips(store: State<'_, SharedStore>) -> Result<Vec<LocalClip>, String> {
    queries::list_clips(&store, None, None, None, None, None, true, 200)
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
    query: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<LocalClip>, String> {
    queries::query_clips(
        &store,
        &query.unwrap_or_default(),
        limit.unwrap_or(50),
        None,
    )
    .map(|v| v.into_iter().map(stored_to_local).collect())
    .map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn search_clips(
    store: State<'_, SharedStore>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<LocalClip>, String> {
    queries::search_clips(&store, &query, limit.unwrap_or(50), None, None)
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

/// Explicitly send an already-captured local clip to the relay (and thus to
/// the user's other devices). This is the ONLY path by which a clip leaves
/// the device — the clipboard monitor never pushes. The clip is broadcast to
/// all of the user's devices.
#[tauri::command]
#[specta::specta]
pub async fn send_clip(pusher: State<'_, LocalPusherHandle>, id: String) -> Result<(), String> {
    let pusher = {
        let guard = pusher
            .lock()
            .map_err(|_| "pusher mutex poisoned".to_string())?;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "not signed in — sign in to enable sending".to_string())?
    };
    pusher
        .send_stored(&id)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
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

/// Edit a clip's text: insert the edited text as a NEW local clip (original
/// untouched), copy it to the clipboard, and return the new clip. The clipboard
/// monitor's `is_self_write` guard suppresses the echo on the next poll tick
/// (with `recent_store_duplicate_id` as a secondary cross-process guard), so
/// the copy does not surface as a duplicate clip.
///
/// If the clipboard write fails the command returns an error even though the
/// new clip is already persisted — local history is append-only, so this is a
/// UI inconvenience, not data loss.
#[tauri::command]
#[specta::specta]
pub fn edit_clip(
    store: State<'_, SharedStore>,
    clipboard: State<'_, Arc<ClipboardService>>,
    original_id: String,
    new_content: String,
) -> Result<LocalClip, String> {
    let new_id = client_core::edit::apply_edit(&store, &original_id, &new_content)
        .map_err(|e| e.to_string())?;
    clipboard
        .write_text(&new_content)
        .map_err(|e| e.to_string())?;
    let row = queries::get_clip(&store, &new_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "edited clip vanished after insert".to_string())?;
    Ok(stored_to_local(row))
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

#[tauri::command]
#[specta::specta]
pub async fn save_image_to_file(
    app: tauri::AppHandle,
    store: State<'_, crate::SharedStore>,
    clip_id: String,
) -> Result<Option<String>, String> {
    // Reject non-image clips defensively: image_bytes_for already enforces
    // this via normalize_content_type, but failing fast with a clear message
    // is better than relying on that helper's exact error string.
    let row = queries::get_clip(&store, &clip_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("clip {} not found", clip_id))?;
    if super::normalize_content_type(row.content_type.clone()) != "image" {
        return Err(format!("clip {} is not an image", clip_id));
    }
    let bytes = image_bytes_for(store.inner(), &clip_id)?;
    let ext = detect_image_ext(&bytes);
    // created_at in client-core is milliseconds; convert to seconds for the filename helper.
    let default_name = default_image_filename(row.created_at / 1000, ext);

    // tauri-plugin-dialog's blocking save dialog must not run on the
    // tokio executor thread; spawn_blocking moves it to the blocking pool.
    let chosen = tokio::task::spawn_blocking({
        let app = app.clone();
        let default_name = default_name.clone();
        let ext_owned = ext.to_string();
        move || {
            app.dialog()
                .file()
                .add_filter("Image", &[ext_owned.as_str()])
                .set_file_name(&default_name)
                .blocking_save_file()
        }
    })
    .await
    .map_err(|e| format!("dialog task panicked: {e}"))?;

    let Some(path) = chosen else {
        return Ok(None);
    };

    // tauri-plugin-dialog returns a `FilePath` enum; turn it into a real
    // PathBuf via `into_path()` (Path variant) and write the bytes.
    let path_buf = path
        .into_path()
        .map_err(|e| format!("dialog returned non-filesystem path: {e}"))?;
    std::fs::write(&path_buf, &bytes).map_err(|e| format!("write failed: {e}"))?;
    Ok(Some(path_buf.to_string_lossy().into_owned()))
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Sniff a few well-known image-format magic bytes. Defaults to "png" for
/// unknown payloads: the desktop push pipeline normalises to PNG, so this
/// is the safest fallback for clipboard images that lack a clearer signal.
fn detect_image_ext(bytes: &[u8]) -> &'static str {
    // Single source of truth for format sniffing; default to "png" since the
    // desktop push pipeline normalises clipboard images to PNG.
    client_core::media::detect_image_format(bytes)
        .map(|f| f.ext())
        .unwrap_or("png")
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

/// Decide what to send from a polled clipboard snapshot. Pure + unit-tested.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SendAction {
    Text(String),
    ImagePng(Vec<u8>),
    Nothing,
}

pub(crate) fn classify_for_send(content: crate::clipboard::backend::PollContent) -> SendAction {
    use crate::clipboard::backend::PollContent;
    match content {
        PollContent::Text(t) if !t.is_empty() => SendAction::Text(t),
        PollContent::ImagePng(b) if !b.is_empty() => SendAction::ImagePng(b),
        _ => SendAction::Nothing,
    }
}

/// Send whatever is currently on the system clipboard to the user's devices.
/// Bound to the opt-in send shortcut. Returns Ok(()) on success, Err on
/// empty/unsupported clipboard or push failure.
#[tauri::command]
#[specta::specta]
pub async fn send_current_clipboard(
    clipboard: State<'_, Arc<ClipboardService>>,
    pusher: State<'_, LocalPusherHandle>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    send_current_clipboard_impl(&clipboard, &pusher, &app).await
}

/// Core of `send_current_clipboard`, decoupled from Tauri `State` so the
/// global-shortcut callback can call it with refs resolved from an `AppHandle`.
///
/// This is an EXPLICIT user-initiated send, so the capture-time bundle-ID
/// exclusion list (`should_accept_snapshot` in the monitor) is intentionally
/// skipped here — it's a frontmost-app heuristic that doesn't apply when the
/// user deliberately fires the hotkey. The stronger content-tagged protection
/// still applies: `poll_snapshot` emits `PollContent::Unsupported` for
/// NSPasteboard concealed/transient items (password managers, 2FA), which
/// `classify_for_send` maps to `SendAction::Nothing` (nothing is sent).
pub(crate) async fn send_current_clipboard_impl(
    clipboard: &ClipboardService,
    pusher: &LocalPusherHandle,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    let snapshot = clipboard.poll_snapshot().map_err(|e| e.to_string())?;
    let pusher = {
        let guard = pusher
            .lock()
            .map_err(|_| "pusher mutex poisoned".to_string())?;
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| "not signed in — sign in to enable sending".to_string())?
    };
    let source = format!("remote:{}", client_core::machine::hostname_or_unknown());
    let ok = match classify_for_send(snapshot.content) {
        SendAction::Text(t) => {
            let raw = t.into_bytes();
            let ct = client_core::classify::detect(&raw);
            pusher
                .push_text(raw, &source, "", ct)
                .await
                .map_err(|e| e.to_string())?;
            true
        }
        SendAction::ImagePng(bytes) => {
            pusher
                .push_image_png(bytes, &source, "")
                .await
                .map_err(|e| e.to_string())?;
            true
        }
        SendAction::Nothing => false,
    };
    let _ = crate::events::ClipSent(ok).emit(app);
    if !ok {
        return Err("clipboard is empty or unsupported".to_string());
    }
    Ok(())
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

#[cfg(test)]
mod send_clip_tests {
    use client_core::store::models::SyncState;
    use client_core::store::queries;
    use std::sync::Arc;

    // The `send_clip` command is a thin wrapper around
    // `LocalPusher::send_stored`. `State` is awkward to construct in a unit
    // test, so we exercise the exact library path the command calls.
    //
    // client-core's recording test client (`RestClient::for_test_recording`,
    // `recorded_pushes`) is `#[cfg(test)]`-only inside client-core, so it is
    // NOT reachable from this (dependent) crate. We instead build an offline
    // client via the public `RestClient::new` aimed at an unreachable port:
    // the push fails with a transient network error, so `send_stored` leaves
    // the clip `Pending` (queued for the backlog flusher to retry). This
    // verifies the same intent with only public client-core API: the path
    // `send_clip` calls actually drives a captured `Local` clip off `Local`.
    #[tokio::test]
    async fn send_clip_path_syncs_local_clip() {
        let store =
            Arc::new(client_core::store::Store::open(std::path::Path::new(":memory:")).unwrap());
        let id =
            client_core::sync::capture::capture_local(&store, "remote:h", "text", b"x".to_vec(), 1)
                .unwrap();
        assert_eq!(
            queries::get_clip(&store, &id).unwrap().unwrap().sync_state,
            SyncState::Local
        );

        // Public offline client: an unreachable localhost port makes every
        // push return a transient network error.
        let client = Arc::new(
            client_core::http::RestClient::new(
                "http://127.0.0.1:1",
                "test-token",
                client_core::version::ClientInfo::for_test(),
            )
            .unwrap(),
        );
        let pusher =
            client_core::sync::LocalPusher::new(store.clone(), client.clone(), Some([7u8; 32]));
        pusher.send_stored(&id).await.unwrap();

        // Transient failure → the clip is queued (Pending), no longer Local.
        assert_eq!(
            queries::get_clip(&store, &id).unwrap().unwrap().sync_state,
            SyncState::Pending
        );
    }
}

#[cfg(test)]
mod edit_tests {
    use client_core::store::models::{StoredClip, SyncState};
    use client_core::store::{queries, Store};
    use std::path::Path;

    #[test]
    fn edit_clip_core_inserts_new_clip_and_keeps_original() {
        // Exercises the shared core the command delegates to. The command
        // itself only adds clipboard write + LocalClip conversion on top.
        let store = Store::open(Path::new(":memory:")).unwrap();
        queries::insert_clip(
            &store,
            &StoredClip {
                id: "01HXFFFFFFFFFFFFFFFFFFFFFF".to_string(),
                source: "local".to_string(),
                content_type: "text".to_string(),
                content: Some(b"![[a.webp|703]]".to_vec()),
                byte_size: 15,
                created_at: 1,
                sync_state: SyncState::Local,
                ..Default::default()
            },
        )
        .unwrap();

        let new_id =
            client_core::edit::apply_edit(&store, "01HXFFFFFFFFFFFFFFFFFFFFFF", "![[a.webp]]")
                .unwrap();
        let new_clip = queries::get_clip(&store, &new_id).unwrap().unwrap();
        assert_eq!(new_clip.content.as_deref(), Some(&b"![[a.webp]]"[..]));
        let original = queries::get_clip(&store, "01HXFFFFFFFFFFFFFFFFFFFFFF")
            .unwrap()
            .unwrap();
        assert_eq!(original.content.as_deref(), Some(&b"![[a.webp|703]]"[..]));
    }
}

#[cfg(test)]
mod send_current_tests {
    use super::*;
    use crate::clipboard::backend::PollContent;

    #[test]
    fn classify_for_send_routes_text_and_image() {
        assert_eq!(
            classify_for_send(PollContent::Text("x".into())),
            SendAction::Text("x".into())
        );
        assert_eq!(
            classify_for_send(PollContent::ImagePng(vec![1, 2])),
            SendAction::ImagePng(vec![1, 2])
        );
        assert_eq!(
            classify_for_send(PollContent::Text(String::new())),
            SendAction::Nothing
        );
        assert_eq!(
            classify_for_send(PollContent::ImagePng(vec![])),
            SendAction::Nothing
        );
        assert_eq!(classify_for_send(PollContent::Empty), SendAction::Nothing);
        assert_eq!(
            classify_for_send(PollContent::Unsupported),
            SendAction::Nothing
        );
    }
}
