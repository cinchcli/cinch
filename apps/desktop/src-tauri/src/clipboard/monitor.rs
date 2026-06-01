//! Clipboard polling loop. Drives `ClipboardService`, applies dedup and the
//! excluded-app filter, then captures clips to LOCAL history via
//! `client_core::sync::capture::capture_local`. The monitor NEVER contacts the
//! relay: a clip leaves the device only via the explicit `send_clip` command.

use log::{error, info, warn};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::sync::Arc;
use tauri::AppHandle;
use tauri_specta::Event;
use tokio::time::{self, Duration};

use super::backend::{PollContent, PollSnapshot};
use super::service::ClipboardService;
use crate::SharedStore;
use client_core::store::settings;

const POLL_INTERVAL: Duration = Duration::from_millis(500);
const DEDUP_WINDOW_SECS: i64 = 5;
const MAX_RECENT_HASHES: usize = 20;
const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024;

/// Pure filter applied in `run_monitor_loop` — returns `false` if the
/// snapshot should be dropped before DB insert.
///
/// Drops when:
/// - content is `Unsupported` (concealed/transient UTI detected upstream in
///   `MacBackend::read_snapshot`) — this is where PRV-01 enforcement lands.
/// - content is `Empty` (nothing to store).
/// - `app_identity` matches any entry in `excluded_apps` (password-manager
///   bundle IDs from `MacBackend::default_excluded_apps`).
///
/// Runs BEFORE self-write dedup (there is no point hashing a clip we'll drop).
pub(crate) fn should_accept_snapshot(snapshot: &PollSnapshot, excluded_apps: &[String]) -> bool {
    if matches!(
        snapshot.content,
        PollContent::Unsupported | PollContent::Empty
    ) {
        return false;
    }
    if let Some(ref bid) = snapshot.app_identity {
        if excluded_apps.iter().any(|e| e == bid) {
            return false;
        }
    }
    true
}

/// What `run_monitor_loop` should do with an accepted, non-self-write
/// snapshot. Pure and unit-tested so the "an in-size image must be pushed,
/// not dropped" contract cannot silently regress again (it did: image clips
/// were detected by the backend but discarded here with only a `warn!`).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ClipAction {
    PushText(String),
    PushImage(Vec<u8>),
    /// Image larger than `max_image_bytes` — carries the size for the log.
    SkipOversizedImage(usize),
    Ignore,
}

/// Decide how to route a snapshot's content. Consumes `content` so the
/// payload moves straight into the handler without a re-clone.
pub(crate) fn classify_snapshot(content: PollContent, max_image_bytes: usize) -> ClipAction {
    match content {
        PollContent::Text(t) if !t.is_empty() => ClipAction::PushText(t),
        PollContent::ImagePng(bytes) if bytes.len() <= max_image_bytes => {
            ClipAction::PushImage(bytes)
        }
        PollContent::ImagePng(bytes) => ClipAction::SkipOversizedImage(bytes.len()),
        PollContent::Text(_) | PollContent::Empty | PollContent::Unsupported => ClipAction::Ignore,
    }
}

struct RecentHash {
    hash: [u8; 32],
    timestamp: i64,
}

pub fn spawn_clipboard_monitor(
    app: &AppHandle,
    service: Arc<ClipboardService>,
    store: SharedStore,
) {
    let app_handle = app.clone();
    tauri::async_runtime::spawn(async move {
        info!("clipboard monitor started (local capture only)");
        run_monitor_loop(&app_handle, &service, &store).await;
    });
}

async fn run_monitor_loop(app: &AppHandle, service: &Arc<ClipboardService>, store: &SharedStore) {
    let mut last_token: Option<u64> = service.token();
    let mut recent_hashes: VecDeque<RecentHash> = VecDeque::new();
    let mut interval = time::interval(POLL_INTERVAL);

    let excluded_apps = load_excluded_apps(store, service);
    let source = format!("remote:{}", client_core::machine::hostname_or_unknown());

    loop {
        interval.tick().await;

        let snapshot = match service.poll_snapshot() {
            Ok(s) => s,
            Err(e) => {
                error!("poll_snapshot failed: {}", e);
                continue;
            }
        };

        // Token fast-path: on platforms with a real change token, skip when
        // nothing has changed. Platforms without a token (Linux) fall through
        // to snapshot-content comparison via the dedup layer.
        if let (Some(cur), Some(last)) = (snapshot.token, last_token) {
            if cur == last {
                continue;
            }
        }
        last_token = snapshot.token;

        // Privacy + excluded-app filter (pure, unit-tested). Runs BEFORE
        // self-write dedup (D-13 ordering) — there is no point hashing a
        // clip we'll drop, and PRV-01 requires that concealed/transient
        // clips never reach later stages. Log excluded-app drops so the
        // operator still sees the signal in the event log.
        if !should_accept_snapshot(&snapshot, &excluded_apps) {
            if let Some(ref bid) = snapshot.app_identity {
                if excluded_apps.iter().any(|e| e == bid) {
                    info!("clipboard change from excluded app: {}", bid);
                }
            }
            continue;
        }

        if service.is_self_write(&snapshot) {
            continue;
        }

        let now = chrono::Utc::now().timestamp();

        let source_app = snapshot.app_name.as_deref();
        let source_app_id = snapshot.app_identity.as_deref();
        let source_url_owned = active_clipboard_source_url(snapshot.app_identity.as_deref());
        let source_url = source_url_owned.as_deref();

        match classify_snapshot(snapshot.content, MAX_IMAGE_BYTES) {
            ClipAction::PushText(text) => {
                handle_text_clip(
                    text,
                    app,
                    store,
                    &mut recent_hashes,
                    now,
                    &source,
                    source_app_id,
                    source_app,
                    source_url,
                );
            }
            ClipAction::PushImage(bytes) => {
                handle_image_clip(
                    bytes,
                    app,
                    store,
                    &mut recent_hashes,
                    now,
                    &source,
                    source_app_id,
                    source_app,
                    source_url,
                );
            }
            ClipAction::SkipOversizedImage(n) => {
                info!("skipping oversized image: {} bytes", n);
            }
            ClipAction::Ignore => {}
        }
    }
}

fn handle_text_clip(
    text: String,
    app: &AppHandle,
    store: &SharedStore,
    recent_hashes: &mut VecDeque<RecentHash>,
    now: i64,
    source: &str,
    source_app_id: Option<&str>,
    source_app: Option<&str>,
    source_url: Option<&str>,
) {
    let hash = compute_hash(text.as_bytes());

    while recent_hashes
        .front()
        .is_some_and(|h| now - h.timestamp > DEDUP_WINDOW_SECS)
    {
        recent_hashes.pop_front();
    }
    if recent_hashes.iter().any(|h| h.hash == hash) {
        return;
    }
    recent_hashes.push_back(RecentHash {
        hash,
        timestamp: now,
    });
    if recent_hashes.len() > MAX_RECENT_HASHES {
        recent_hashes.pop_front();
    }

    let raw = text.into_bytes();
    let content_type = client_core::classify::detect(&raw);
    let byte_size = raw.len() as i64;
    match client_core::sync::capture::capture_local_with_metadata(
        store,
        source,
        source_app_id,
        source_app,
        source_url,
        content_type.as_wire(),
        raw,
        byte_size,
    ) {
        Ok(clip_id) => {
            info!(
                "clipboard: captured text clip {} locally ({} bytes)",
                clip_id, byte_size
            );
            let payload = clip_received_stub(
                &clip_id,
                source,
                source_app_id,
                source_app,
                source_url,
                byte_size,
                content_type.as_wire(),
            );
            let _ = crate::events::ClipReceived(payload).emit(app);
        }
        Err(e) => warn!("clipboard: local capture failed: {}", e),
    }
}

/// Image-clip analog of `handle_text_clip`. Same dedup window; captures the
/// PNG/TIFF bytes to LOCAL history via `capture_local` as
/// `content_type = "image"`. The relay is never contacted.
fn handle_image_clip(
    bytes: Vec<u8>,
    app: &AppHandle,
    store: &SharedStore,
    recent_hashes: &mut VecDeque<RecentHash>,
    now: i64,
    source: &str,
    source_app_id: Option<&str>,
    source_app: Option<&str>,
    source_url: Option<&str>,
) {
    let hash = compute_hash(&bytes);

    while recent_hashes
        .front()
        .is_some_and(|h| now - h.timestamp > DEDUP_WINDOW_SECS)
    {
        recent_hashes.pop_front();
    }
    if recent_hashes.iter().any(|h| h.hash == hash) {
        return;
    }
    recent_hashes.push_back(RecentHash {
        hash,
        timestamp: now,
    });
    if recent_hashes.len() > MAX_RECENT_HASHES {
        recent_hashes.pop_front();
    }

    let byte_size = bytes.len() as i64;
    let content_type = client_core::rest::ContentType::Image.as_wire();
    match client_core::sync::capture::capture_local_with_metadata(
        store,
        source,
        source_app_id,
        source_app,
        source_url,
        content_type,
        bytes,
        byte_size,
    ) {
        Ok(clip_id) => {
            info!(
                "clipboard: captured image clip {} locally ({} bytes)",
                clip_id, byte_size
            );
            let payload = clip_received_stub(
                &clip_id,
                source,
                source_app_id,
                source_app,
                source_url,
                byte_size,
                content_type,
            );
            let _ = crate::events::ClipReceived(payload).emit(app);
        }
        Err(e) => warn!("clipboard: local capture failed: {}", e),
    }
}

/// Build a minimal `LocalClip` payload for the `ClipReceived` event. The React
/// listener uses `source` to look up per-device alert settings and otherwise
/// treats the payload as a refresh trigger (it re-fetches via `list_clips` in
/// the handler), so the other fields are stubbed.
pub(crate) fn clip_received_stub(
    clip_id: &str,
    source: &str,
    source_app_id: Option<&str>,
    source_app: Option<&str>,
    source_url: Option<&str>,
    byte_size: i64,
    content_type: &str,
) -> crate::commands::clips::LocalClip {
    let now_secs = chrono::Utc::now().timestamp();
    crate::commands::clips::LocalClip {
        id: clip_id.to_string(),
        user_id: String::new(),
        content: String::new(),
        // Defensive: legacy WebSocket-relayed clips may arrive with MIME-style
        // content_type ("text/plain", "image/png") from pre-0510e1f desktop
        // builds. Collapse to the canonical 4-string vocab so the React side
        // can keep using strict equality.
        content_type: crate::commands::clips::normalize_content_type(content_type.to_string()),
        source: source.to_string(),
        source_app_id: source_app_id.map(str::to_string),
        source_app: source_app.map(str::to_string),
        source_url: source_url.map(str::to_string),
        label: String::new(),
        byte_size,
        media_path: None,
        created_at: now_secs,
        synced: false,
        sync_state: "local".to_string(),
        is_pinned: false,
        pin_note: None,
        received_at: now_secs,
    }
}

fn load_excluded_apps(store: &SharedStore, service: &ClipboardService) -> Vec<String> {
    match settings::excluded_apps(store) {
        Ok(apps) if !apps.is_empty() => apps,
        _ => {
            let defaults = service.default_excluded_apps();
            // Persist the defaults on first run so future reads return them.
            settings::set_excluded_apps(store, &defaults).ok();
            defaults
        }
    }
}

fn compute_hash(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}

fn active_clipboard_source_url(bundle_id: Option<&str>) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        bundle_id.and_then(crate::clipboard::backend::macos::active_browser_url_for_bundle)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = bundle_id;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_hash_deterministic() {
        let h1 = compute_hash(b"hello world");
        let h2 = compute_hash(b"hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_compute_hash_different_content() {
        let h1 = compute_hash(b"hello");
        let h2 = compute_hash(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_dedup_window_logic() {
        let hash = compute_hash(b"test content");
        let now = chrono::Utc::now().timestamp();
        let mut recent: VecDeque<RecentHash> = VecDeque::new();
        recent.push_back(RecentHash {
            hash,
            timestamp: now,
        });
        assert!(recent.iter().any(|h| h.hash == hash));
        let other_hash = compute_hash(b"different content");
        assert!(!recent.iter().any(|h| h.hash == other_hash));
    }

    #[test]
    fn test_dedup_window_expiry() {
        let hash = compute_hash(b"test");
        let now = chrono::Utc::now().timestamp();
        let mut recent: VecDeque<RecentHash> = VecDeque::new();
        recent.push_back(RecentHash {
            hash,
            timestamp: now - 10,
        });
        while recent
            .front()
            .is_some_and(|h| now - h.timestamp > DEDUP_WINDOW_SECS)
        {
            recent.pop_front();
        }
        assert!(recent.is_empty());
    }

    // --- Wave 0 tests for `should_accept_snapshot` filter (plan 01-04) ---

    #[test]
    fn should_accept_snapshot_drops_concealed() {
        let snap = PollSnapshot {
            token: Some(1),
            content: PollContent::Unsupported,
            app_identity: None,
            app_name: None,
        };
        assert!(
            !should_accept_snapshot(&snap, &[]),
            "Unsupported content (concealed/transient UTI) must be dropped"
        );
    }

    #[test]
    fn should_accept_snapshot_drops_empty() {
        let snap = PollSnapshot {
            token: Some(1),
            content: PollContent::Empty,
            app_identity: Some("com.apple.TextEdit".into()),
            app_name: Some("TextEdit".into()),
        };
        assert!(
            !should_accept_snapshot(&snap, &[]),
            "Empty content must be dropped"
        );
    }

    #[test]
    fn should_accept_snapshot_drops_excluded_app() {
        let snap = PollSnapshot {
            token: Some(1),
            content: PollContent::Text("secret".into()),
            app_identity: Some("com.1password.1password".into()),
            app_name: Some("1Password".into()),
        };
        assert!(
            !should_accept_snapshot(&snap, &["com.1password.1password".into()]),
            "Clip from excluded bundle ID must be dropped"
        );
    }

    #[test]
    fn should_accept_snapshot_accepts_normal_text() {
        let snap = PollSnapshot {
            token: Some(1),
            content: PollContent::Text("hello".into()),
            app_identity: Some("com.apple.TextEdit".into()),
            app_name: Some("TextEdit".into()),
        };
        assert!(
            should_accept_snapshot(&snap, &["com.1password.1password".into()]),
            "Normal clip from non-excluded app must be accepted"
        );
    }

    #[test]
    fn should_accept_snapshot_accepts_image() {
        let snap = PollSnapshot {
            token: Some(1),
            content: PollContent::ImagePng(vec![0x89, 0x50, 0x4E, 0x47]),
            app_identity: None,
            app_name: None,
        };
        assert!(
            should_accept_snapshot(&snap, &[]),
            "ImagePng with no excluded app_identity must be accepted"
        );
    }

    // --- `classify_snapshot` routing contract ---
    //
    // Regression guard for the bug where an image clip was detected by the
    // backend (`should_accept_snapshot` even accepts it, above) but then
    // silently dropped by `run_monitor_loop` instead of being pushed.

    #[test]
    fn classify_snapshot_routes_in_size_image_to_push() {
        let bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        assert_eq!(
            classify_snapshot(PollContent::ImagePng(bytes.clone()), MAX_IMAGE_BYTES),
            ClipAction::PushImage(bytes),
            "an in-size image must be routed to the push path, not dropped"
        );
    }

    #[test]
    fn classify_snapshot_routes_oversized_image_to_skip() {
        let big = vec![0u8; MAX_IMAGE_BYTES + 1];
        assert_eq!(
            classify_snapshot(PollContent::ImagePng(big), MAX_IMAGE_BYTES),
            ClipAction::SkipOversizedImage(MAX_IMAGE_BYTES + 1),
        );
    }

    #[test]
    fn classify_snapshot_routes_text_to_push() {
        assert_eq!(
            classify_snapshot(PollContent::Text("hello".into()), MAX_IMAGE_BYTES),
            ClipAction::PushText("hello".into()),
        );
    }

    #[test]
    fn classify_snapshot_empty_text_is_ignore() {
        assert_eq!(
            classify_snapshot(PollContent::Text(String::new()), MAX_IMAGE_BYTES),
            ClipAction::Ignore,
        );
    }

    #[test]
    fn classify_snapshot_unsupported_and_empty_are_ignore() {
        assert_eq!(
            classify_snapshot(PollContent::Unsupported, MAX_IMAGE_BYTES),
            ClipAction::Ignore,
        );
        assert_eq!(
            classify_snapshot(PollContent::Empty, MAX_IMAGE_BYTES),
            ClipAction::Ignore,
        );
    }
}
