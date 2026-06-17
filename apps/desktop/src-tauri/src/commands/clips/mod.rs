use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::protocol::MultiConfigHandle;
use client_core::store::models::{SourceRow, StoredClip};
use client_core::store::queries;

mod action_shortcut;
mod agent_resume;
mod clip_ops;
mod device_cache;
mod devices;
mod global_shortcut;
mod misc;
mod retention;
mod source_settings;

pub use action_shortcut::*;
pub use agent_resume::*;
pub use clip_ops::*;
pub use device_cache::{DeviceCache, DeviceCacheHandle};
pub use devices::*;
pub use global_shortcut::*;
pub use misc::*;
pub use retention::*;
pub use source_settings::*;

// ---------------------------------------------------------------------------
// Local wire type kept for Specta / frontend compatibility.
//
// `StoredClip` from client_core uses `content: Option<Vec<u8>>` (binary-safe).
// The frontend was built against `LocalClip` (String content + extra metadata).
// Rather than updating every .tsx file in this task, we keep this shape and
// convert with `stored_to_local` below.
//
// TODO(phase 5): migrate the frontend to consume StoredClip directly and
// delete LocalClip from here.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LocalClip {
    pub id: String,
    pub user_id: String,
    pub content: String,
    pub content_type: String,
    pub source: String,
    pub source_app_id: Option<String>,
    pub source_app: Option<String>,
    pub source_url: Option<String>,
    pub label: String,
    pub byte_size: i64,
    pub media_path: Option<String>,
    pub created_at: i64, // unix seconds (frontend convention)
    pub synced: bool,
    pub sync_state: String,
    pub is_pinned: bool,
    pub pin_note: Option<String>,
    pub received_at: i64,
}

// ---------------------------------------------------------------------------
// SourceInfo — returned to the frontend; matches the old desktop shape.
// client_core::store::models::SourceRow has the same fields (source,
// clip_count, last_seen) so we forward it directly but keep the desktop name
// for Specta compatibility.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SourceInfo {
    pub source: String,
    pub clip_count: i64,
    pub last_seen: i64,
}

/// Normalize a stored `content_type` to the canonical 4-string wire vocab.
///
/// Pre-0510e1f desktop builds emitted MIME-style values (`"text/plain"`,
/// `"image/png"`) and the relay's open-string column preserved them. The
/// frontend dispatches on strict equality (`=== "image"`, `=== "text"`),
/// so we collapse MIME prefixes here — at the Rust → frontend boundary —
/// rather than spreading the defense across every React component.
pub(crate) fn normalize_content_type(ct: String) -> String {
    // Single source of truth lives in client-core; kept as a thin wrapper
    // because this is the documented Rust→frontend boundary helper.
    client_core::rest::normalize_content_type(&ct)
}

/// Convert a `StoredClip` (client-core, ms timestamps) to a `LocalClip`
/// (desktop frontend, second timestamps).
fn stored_to_local(c: StoredClip) -> LocalClip {
    let content = c
        .content
        .as_deref()
        .and_then(|b| std::str::from_utf8(b).ok())
        .unwrap_or("")
        .to_string();
    // client-core stores created_at in milliseconds; frontend expects seconds.
    let created_at_secs = c.created_at / 1000;
    LocalClip {
        id: c.id,
        user_id: String::new(),
        content,
        content_type: normalize_content_type(c.content_type),
        source: c.source,
        source_app_id: c.source_app_id,
        source_app: c.source_app,
        source_url: c.source_url,
        label: c.label.unwrap_or_default(),
        byte_size: c.byte_size,
        media_path: c.media_path,
        created_at: created_at_secs,
        synced: matches!(c.sync_state, client_core::store::models::SyncState::Synced),
        sync_state: c.sync_state.as_str().to_string(),
        is_pinned: c.pinned,
        pin_note: None, // pinned_at is an i64 in StoredClip; notes not stored
        received_at: created_at_secs,
    }
}

fn source_row_to_info(r: SourceRow) -> SourceInfo {
    SourceInfo {
        source: r.source,
        clip_count: r.clip_count,
        // client-core stores last_seen in milliseconds; convert to seconds.
        last_seen: r.last_seen.unwrap_or(0) / 1000,
    }
}

/// Read an image clip's raw bytes from the store. Err if absent / not an image.
pub(crate) fn image_bytes_for(
    store: &client_core::store::Store,
    clip_id: &str,
) -> Result<Vec<u8>, String> {
    match queries::get_clip(store, clip_id) {
        Ok(Some(c)) if normalize_content_type(c.content_type.clone()) == "image" => c
            .content
            .filter(|b| !b.is_empty())
            .ok_or_else(|| "clip has no image bytes".to_string()),
        Ok(_) => Err(format!("no image clip with id {clip_id}")),
        Err(e) => Err(e.to_string()),
    }
}

/// Helper: extract (relay_url, token) from active MultiConfig profile.
fn resolve_active_creds(mc: &State<'_, MultiConfigHandle>) -> Result<(String, String), String> {
    let guard = mc.lock().unwrap();
    let profile = guard.active_profile().ok_or("no active relay configured")?;
    let token = if profile.token.is_empty() {
        let cfg = profile.to_config();
        crate::auth::read_credentials(&cfg).map_err(|_| "not authenticated".to_string())?
    } else {
        profile.token.clone()
    };
    if token.is_empty() {
        return Err("not authenticated".to_string());
    }
    Ok((profile.relay_url.clone(), token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_core::store::models::SyncState;

    #[test]
    fn stored_to_local_converts_ms_to_seconds() {
        let sc = StoredClip {
            id: "01JTEST00000000000000000000".to_string(),
            source: "local".to_string(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "text".to_string(),
            content: Some(b"hello".to_vec()),
            media_path: None,
            byte_size: 5,
            created_at: 1_777_614_529_000, // ms
            pinned: false,
            pinned_at: None,
            sync_state: SyncState::Synced,
        };
        let lc = stored_to_local(sc);
        assert_eq!(lc.created_at, 1_777_614_529); // seconds
        assert_eq!(lc.content, "hello");
        assert!(!lc.is_pinned);
    }

    #[test]
    fn stored_to_local_binary_content_is_empty_string() {
        let sc = StoredClip {
            id: "01JTEST00000000000000000001".to_string(),
            source: "local".to_string(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "image".to_string(),
            content: None,
            media_path: Some("media/shot.png".to_string()),
            byte_size: 1024,
            created_at: 1_000_000_000,
            pinned: false,
            pinned_at: None,
            sync_state: SyncState::Synced,
        };
        let lc = stored_to_local(sc);
        assert_eq!(lc.content, "");
        assert_eq!(lc.media_path.as_deref(), Some("media/shot.png"));
    }

    #[test]
    fn stored_to_local_carries_sync_state() {
        let sc = StoredClip {
            id: "01JTEST00000000000000000003".to_string(),
            source: "remote:host".to_string(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "text".to_string(),
            content: Some(b"hi".to_vec()),
            media_path: None,
            byte_size: 2,
            created_at: 1_700_000_000_000,
            pinned: false,
            pinned_at: None,
            sync_state: client_core::store::models::SyncState::Pending,
        };
        let lc = stored_to_local(sc);
        assert_eq!(lc.sync_state, "pending");
    }

    #[test]
    fn normalize_content_type_collapses_mime_prefixes() {
        // Canonical 4-string vocab passes through unchanged.
        assert_eq!(normalize_content_type("text".into()), "text");
        assert_eq!(normalize_content_type("code".into()), "code");
        assert_eq!(normalize_content_type("url".into()), "url");
        assert_eq!(normalize_content_type("image".into()), "image");
        // Legacy MIME values from pre-0510e1f desktop builds collapse.
        assert_eq!(normalize_content_type("text/plain".into()), "text");
        assert_eq!(normalize_content_type("text/html".into()), "text");
        assert_eq!(normalize_content_type("image/png".into()), "image");
        assert_eq!(normalize_content_type("image/jpeg".into()), "image");
        // Unknown values flow through verbatim — defense, not censorship.
        assert_eq!(normalize_content_type("audio".into()), "audio");
        assert_eq!(normalize_content_type("".into()), "");
    }

    #[test]
    fn stored_to_local_normalizes_legacy_mime() {
        let sc = StoredClip {
            id: "01JTEST00000000000000000002".to_string(),
            source: "remote:laptop".to_string(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "text/plain".to_string(),
            content: Some(b"legacy".to_vec()),
            media_path: None,
            byte_size: 6,
            created_at: 1_700_000_000_000,
            pinned: false,
            pinned_at: None,
            sync_state: SyncState::Synced,
        };
        let lc = stored_to_local(sc);
        assert_eq!(
            lc.content_type, "text",
            "MIME-style stored values must surface as canonical to the frontend"
        );
    }

    #[test]
    fn image_bytes_for_returns_image_row_bytes() {
        let s = client_core::store::Store::open(std::path::Path::new(":memory:")).unwrap();
        let png = vec![0x89u8, 0x50, 0x4E, 0x47];
        client_core::store::queries::insert_clip(
            &s,
            &StoredClip {
                id: "i1".into(),
                source: "remote:t".into(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: "image".into(),
                content: Some(png.clone()),
                media_path: None,
                byte_size: 0,
                created_at: 1,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Synced,
            },
        )
        .unwrap();
        assert_eq!(image_bytes_for(&s, "i1").unwrap(), png);
        assert!(image_bytes_for(&s, "missing").is_err());
    }

    #[test]
    fn image_bytes_for_accepts_legacy_mime_content_type() {
        let s = client_core::store::Store::open(std::path::Path::new(":memory:")).unwrap();
        let png = vec![0x89u8, 0x50, 0x4E, 0x47];
        client_core::store::queries::insert_clip(
            &s,
            &StoredClip {
                id: "legacy1".into(),
                source: "remote:peer".into(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: "image/png".into(),
                content: Some(png.clone()),
                media_path: None,
                byte_size: 0,
                created_at: 1,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Synced,
            },
        )
        .unwrap();
        assert_eq!(image_bytes_for(&s, "legacy1").unwrap(), png);
    }
}
