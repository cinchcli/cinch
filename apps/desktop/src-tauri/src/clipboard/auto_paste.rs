//! Sync incoming remote clips into the local OS pasteboard so paste works
//! immediately in any app. Currently scoped to images — text/code/url remain
//! manual Copy-button only, so the user can browse history without losing
//! whatever text they just put on their local clipboard.

use client_core::store::Store;

use crate::clipboard::ClipboardService;
use crate::commands::clips::image_bytes_for;

/// True when the wire content_type is an image. Accepts both the canonical
/// "image" and pre-2026-05 MIME-style "image/*" so legacy clips still trigger.
fn is_image(content_type: &str) -> bool {
    content_type.starts_with("image")
}

/// If the incoming clip is an image, look up its raw PNG bytes from the local
/// store and write them to the OS pasteboard. Best-effort: errors are logged
/// at warn-level so a malformed clip never crashes the WS drainer.
pub fn paste_incoming_image(
    clipboard: &ClipboardService,
    store: &Store,
    clip_id: &str,
    content_type: &str,
) {
    if !is_image(content_type) {
        return;
    }
    let bytes = match image_bytes_for(store, clip_id) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("auto-paste: store lookup failed for {clip_id}: {e}");
            return;
        }
    };
    if let Err(e) = clipboard.write_image_png_bytes(&bytes) {
        log::warn!("auto-paste: clipboard write failed for {clip_id}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::backend::{Backend, ClipboardError, PollContent, PollSnapshot};
    use client_core::store::{
        models::{StoredClip, SyncState},
        queries,
    };
    use std::path::Path;

    const PNG: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    struct StubBackend {
        token: u64,
    }

    impl Backend for StubBackend {
        fn read_snapshot(&mut self) -> Result<PollSnapshot, ClipboardError> {
            Ok(PollSnapshot {
                token: Some(self.token),
                content: PollContent::Empty,
                app_identity: None,
                app_name: None,
            })
        }
        fn write_text(&mut self, _: &str) -> Result<(), ClipboardError> {
            self.token += 1;
            Ok(())
        }
        fn write_image_png(&mut self, _: &[u8]) -> Result<(), ClipboardError> {
            self.token += 1;
            Ok(())
        }
        fn default_excluded_apps(&self) -> Vec<String> {
            vec![]
        }
    }

    fn fresh_service() -> ClipboardService {
        ClipboardService::new_with_backend(Box::new(StubBackend { token: 0 }))
    }

    fn store_with_image(id: &str, ct: &str, bytes: &[u8]) -> Store {
        let s = Store::open(Path::new(":memory:")).unwrap();
        queries::insert_clip(
            &s,
            &StoredClip {
                id: id.into(),
                source: "remote:test".into(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: ct.into(),
                content: Some(bytes.to_vec()),
                media_path: None,
                byte_size: bytes.len() as i64,
                created_at: 1,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Synced,
            },
        )
        .unwrap();
        s
    }

    fn expected_self_write(bytes: &[u8]) -> PollSnapshot {
        PollSnapshot {
            token: Some(u64::MAX),
            content: PollContent::ImagePng(bytes.to_vec()),
            app_identity: None,
            app_name: None,
        }
    }

    #[test]
    fn writes_png_for_canonical_image_content_type() {
        let store = store_with_image("a", "image", &PNG);
        let svc = fresh_service();
        paste_incoming_image(&svc, &store, "a", "image");
        assert!(
            svc.is_self_write(&expected_self_write(&PNG)),
            "image clip must be recorded as a self-write after paste"
        );
    }

    #[test]
    fn writes_png_for_legacy_image_mime() {
        // Pre-2026-05 wire form. Wire `content_type` is open string, so the
        // auto-paste boundary must handle MIME-style values for older relays.
        let store = store_with_image("b", "image/png", &PNG);
        let svc = fresh_service();
        paste_incoming_image(&svc, &store, "b", "image/png");
        assert!(svc.is_self_write(&expected_self_write(&PNG)));
    }

    #[test]
    fn text_content_type_does_not_touch_clipboard() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let svc = fresh_service();
        paste_incoming_image(&svc, &store, "c", "text");
        // No write ⇒ no self-write record, so any synthesized image snapshot
        // must look external.
        assert!(!svc.is_self_write(&expected_self_write(&PNG)));
    }

    #[test]
    fn missing_image_row_is_silently_skipped() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let svc = fresh_service();
        paste_incoming_image(&svc, &store, "no-such-clip", "image");
        assert!(!svc.is_self_write(&expected_self_write(&PNG)));
    }

    #[test]
    fn empty_image_row_is_silently_skipped() {
        let s = Store::open(Path::new(":memory:")).unwrap();
        queries::insert_clip(
            &s,
            &StoredClip {
                id: "empty".into(),
                source: "remote:test".into(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: "image".into(),
                content: None,
                media_path: None,
                byte_size: 0,
                created_at: 1,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Synced,
            },
        )
        .unwrap();
        let svc = fresh_service();
        paste_incoming_image(&svc, &s, "empty", "image");
        assert!(!svc.is_self_write(&expected_self_write(&PNG)));
    }
}
