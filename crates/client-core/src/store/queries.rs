//! Facade over the focused store-query modules.
//!
//! Historically every clip/device/retention/sync query lived in this one
//! file. The implementations now live in sibling modules — [`clips`],
//! [`search`], [`devices`], [`retention`], and [`sync_state`] — and this
//! module re-exports them so existing `store::queries::*` call sites keep
//! working unchanged. New code may depend on the focused modules directly.
//!
//! [`clips`]: super::clips
//! [`search`]: super::search
//! [`devices`]: super::devices
//! [`retention`]: super::retention
//! [`sync_state`]: super::sync_state

pub use super::clips::{
    clear_all_clips, clip_count, count_clips_before, delete_clip, get_clip, insert_clip,
    list_clips, purge_clips_before, recent_clip_id_by_content, set_pinned,
};
pub use super::devices::{list_devices, list_sources};
pub use super::retention::{list_retention, set_retention};
pub use super::search::{
    parse_query_string, query_clips, sanitize_fts_query, search_clips, ParsedQuery,
};
pub use super::sync_state::{
    enforce_offline_cap, list_pending_clips, mark_local, mark_pending, replace_id_and_mark_synced,
    set_watermark, watermark,
};

// `Store` is re-exported privately so the integration-style tests below can
// reach it through `use super::*`.
#[cfg(test)]
use super::Store;

#[cfg(test)]
mod tests {
    use super::super::models::{StoredClip, SyncState};
    use super::*;

    #[test]
    fn recent_clip_id_by_content_matches_within_window_only() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mk = |id: &str, content: &[u8], created_at: i64| StoredClip {
            id: id.into(),
            source: "atlas0".into(),
            content_type: "text".into(),
            content: Some(content.to_vec()),
            byte_size: content.len() as i64,
            created_at,
            sync_state: SyncState::Pending,
            ..Default::default()
        };
        // A clip saved "now" plus an identical-content clip saved long ago.
        insert_clip(&store, &mk("recent", b"## Assistant\n\nhi", 10_000)).unwrap();
        insert_clip(&store, &mk("old", b"old content", 1_000)).unwrap();

        // Byte-identical content created at/after since_ms → found (the echo guard).
        let hit = recent_clip_id_by_content(&store, b"## Assistant\n\nhi", 5_000).unwrap();
        assert_eq!(hit.as_deref(), Some("recent"));

        // Same content, but the only match predates since_ms → ignored.
        let miss_old = recent_clip_id_by_content(&store, b"old content", 5_000).unwrap();
        assert_eq!(
            miss_old, None,
            "matches older than since_ms must be ignored"
        );

        // Content that was never stored → no match.
        let miss_diff = recent_clip_id_by_content(&store, b"never stored", 0).unwrap();
        assert_eq!(miss_diff, None);
    }

    #[test]
    fn query_clips_with_from_filter_does_not_deadlock() {
        // Regression for the C1 deadlock: query_clips used to call
        // store.with_conn() a SECOND time (to resolve `from:` device
        // nicknames) while already holding the connection lock, deadlocking
        // the non-reentrant Mutex<Connection>. Any `from:` query must now
        // return promptly. (If this regresses, the test hangs → CI timeout.)
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        insert_clip(
            &store,
            &StoredClip {
                id: "c-from".into(),
                source: "deviceA".into(),
                content_type: "text".into(),
                content: Some(b"hello".to_vec()),
                byte_size: 5,
                created_at: 1,
                sync_state: SyncState::Synced,
                ..Default::default()
            },
        )
        .unwrap();

        // Exact-source match (the device-resolution path falls through to it).
        let hits = query_clips(&store, "from:deviceA", 10, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "c-from");

        // A `from:` value with no match also returns promptly (no hang).
        assert!(query_clips(&store, "from:nope", 10, None)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn insert_clip_persists_sync_state() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let clip = StoredClip {
            id: "01HXABC".into(),
            source: "atlas0".into(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "text".into(),
            content: Some(b"hello".to_vec()),
            media_path: None,
            byte_size: 5,
            created_at: 1_700_000_000_000,
            pinned: false,
            pinned_at: None,
            sync_state: SyncState::Pending,
        };
        insert_clip(&store, &clip).unwrap();
        let row = get_clip(&store, &clip.id).unwrap().unwrap();
        assert_eq!(
            row.sync_state,
            SyncState::Pending,
            "sync_state=Pending must survive an insert/read round-trip"
        );
    }

    // ── Task 5: list_pending_clips ───────────────────────────────────────────

    #[test]
    fn list_pending_clips_excludes_local_and_synced() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        fn make(id: &str, ts: i64, state: SyncState) -> StoredClip {
            StoredClip {
                id: id.into(),
                source: "s".into(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: "text".into(),
                content: Some(b"x".to_vec()),
                media_path: None,
                byte_size: 1,
                created_at: ts,
                pinned: false,
                pinned_at: None,
                sync_state: state,
            }
        }
        for c in [
            make("local", 10, SyncState::Local),
            make("pending", 20, SyncState::Pending),
            make("synced", 30, SyncState::Synced),
        ] {
            insert_clip(&store, &c).unwrap();
        }
        let ids: Vec<String> = list_pending_clips(&store)
            .unwrap()
            .into_iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(ids, vec!["pending".to_string()]);
    }

    #[test]
    fn mark_pending_and_mark_local_transition() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let c = StoredClip {
            id: "c1".into(),
            source: "s".into(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "text".into(),
            content: Some(b"x".to_vec()),
            media_path: None,
            byte_size: 1,
            created_at: 0,
            pinned: false,
            pinned_at: None,
            sync_state: SyncState::Local,
        };
        insert_clip(&store, &c).unwrap();
        mark_pending(&store, "c1").unwrap();
        assert_eq!(
            get_clip(&store, "c1").unwrap().unwrap().sync_state,
            SyncState::Pending
        );
        mark_local(&store, "c1").unwrap();
        assert_eq!(
            get_clip(&store, "c1").unwrap().unwrap().sync_state,
            SyncState::Local
        );
    }

    #[test]
    fn list_pending_clips_returns_oldest_first() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        fn make(id: &str, ts: i64, sync_state: SyncState) -> StoredClip {
            StoredClip {
                id: id.into(),
                source: "s".into(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: "text".into(),
                content: Some(b"x".to_vec()),
                media_path: None,
                byte_size: 1,
                created_at: ts,
                pinned: false,
                pinned_at: None,
                sync_state,
            }
        }
        for c in [
            make("a", 30, SyncState::Synced),
            make("b", 10, SyncState::Pending),
            make("c", 20, SyncState::Pending),
        ] {
            insert_clip(&store, &c).unwrap();
        }
        let rows = list_pending_clips(&store).unwrap();
        let ids: Vec<&str> = rows.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "c"]);
    }

    // ── Task 6: enforce_offline_cap ─────────────────────────────────────────

    #[test]
    fn enforce_offline_cap_drops_oldest_unsynced() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        for (id, ts) in [("a", 10i64), ("b", 20), ("c", 30)] {
            let c = StoredClip {
                id: id.into(),
                source: "s".into(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: "text".into(),
                content: Some(b"x".to_vec()),
                media_path: None,
                byte_size: 1,
                created_at: ts,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Pending,
            };
            insert_clip(&store, &c).unwrap();
        }
        let dropped = enforce_offline_cap(&store, 2).unwrap();
        assert_eq!(dropped, 1);
        let remaining = list_pending_clips(&store).unwrap();
        let ids: Vec<&str> = remaining.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "c"]);
    }

    #[test]
    fn enforce_offline_cap_is_noop_when_under_cap() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let c = StoredClip {
            id: "a".into(),
            source: "s".into(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "text".into(),
            content: Some(b"x".to_vec()),
            media_path: None,
            byte_size: 1,
            created_at: 0,
            pinned: false,
            pinned_at: None,
            sync_state: SyncState::Pending,
        };
        insert_clip(&store, &c).unwrap();
        let dropped = enforce_offline_cap(&store, 10).unwrap();
        assert_eq!(dropped, 0);
    }

    // ── Task 7: replace_id_and_mark_synced ──────────────────────────────────

    #[test]
    fn replace_id_and_mark_synced_swaps_id_and_flag() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let c = StoredClip {
            id: "local-01H".into(),
            source: "s".into(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "text".into(),
            content: Some(b"x".to_vec()),
            media_path: None,
            byte_size: 1,
            created_at: 0,
            pinned: false,
            pinned_at: None,
            sync_state: SyncState::Pending,
        };
        insert_clip(&store, &c).unwrap();
        let n = replace_id_and_mark_synced(&store, "local-01H", "01HRELAYID").unwrap();
        assert_eq!(n, 1);
        assert!(get_clip(&store, "local-01H").unwrap().is_none());
        let after = get_clip(&store, "01HRELAYID").unwrap().unwrap();
        assert_eq!(after.sync_state, SyncState::Synced);
    }

    #[test]
    fn replace_id_and_mark_synced_is_benign_when_row_missing() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let n = replace_id_and_mark_synced(&store, "local-gone", "01HNEW").unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn replace_id_and_mark_synced_merges_when_target_exists() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let old = StoredClip {
            id: "local-01H".into(),
            source: "s".into(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "text".into(),
            content: Some(b"x".to_vec()),
            media_path: None,
            byte_size: 1,
            created_at: 0,
            pinned: true,
            pinned_at: Some(10),
            sync_state: SyncState::Pending,
        };
        let target = StoredClip {
            id: "01HRELAYID".into(),
            source: "s".into(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "text".into(),
            content: Some(b"x".to_vec()),
            media_path: None,
            byte_size: 1,
            created_at: 0,
            pinned: false,
            pinned_at: None,
            sync_state: SyncState::Synced,
        };
        insert_clip(&store, &old).unwrap();
        insert_clip(&store, &target).unwrap();

        let n = replace_id_and_mark_synced(&store, "local-01H", "01HRELAYID").unwrap();
        assert_eq!(n, 1);
        assert!(get_clip(&store, "local-01H").unwrap().is_none());

        let after = get_clip(&store, "01HRELAYID").unwrap().unwrap();
        assert_eq!(after.sync_state, SyncState::Synced);
        assert!(after.pinned);
        assert_eq!(after.pinned_at, Some(10));
    }

    #[test]
    fn search_clips_prioritizes_labels() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        // 1. A very recent clip with "api" in content
        insert_clip(
            &store,
            &StoredClip {
                id: "recent-content".into(),
                source: "s".into(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: "text".into(),
                content: Some(b"this is some api content".to_vec()),
                media_path: None,
                byte_size: 24,
                created_at: 1000,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Synced,
            },
        )
        .unwrap();

        // 2. An older clip with "api" in label
        insert_clip(
            &store,
            &StoredClip {
                id: "older-label".into(),
                source: "s".into(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: Some("api-v1".into()),
                content_type: "text".into(),
                content: Some(b"nothing".to_vec()),
                media_path: None,
                byte_size: 7,
                created_at: 500,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Synced,
            },
        )
        .unwrap();

        let hits = search_clips(&store, "api", 10, None, None).unwrap();
        assert_eq!(hits.len(), 2);
        // Even though it's older, the label match should come first.
        assert_eq!(
            hits[0].id, "older-label",
            "Label match should be prioritized over content match"
        );
        assert_eq!(hits[1].id, "recent-content");
    }

    #[test]
    fn search_clips_excludes_images() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        // Insert a text clip containing "needle"
        insert_clip(
            &store,
            &StoredClip {
                id: "text-1".into(),
                source: "s".into(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: "text".into(),
                content: Some(b"this is a needle".to_vec()),
                media_path: None,
                byte_size: 16,
                created_at: 10,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Synced,
            },
        )
        .unwrap();

        // Insert an image clip whose raw bytes happen to contain "needle"
        insert_clip(
            &store,
            &StoredClip {
                id: "image-1".into(),
                source: "s".into(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: "image/png".into(),
                content: Some(b"binary data with needle here".to_vec()),
                media_path: None,
                byte_size: 28,
                created_at: 20,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Synced,
            },
        )
        .unwrap();

        let hits = search_clips(&store, "needle", 10, None, None).unwrap();
        // BEFORE FIX: this will likely be 2.
        // AFTER FIX: this should be 1 (only text-1).
        assert_eq!(
            hits.len(),
            1,
            "Should only find text clip, not image clip. Found: {:?}",
            hits.iter().map(|c| &c.id).collect::<Vec<_>>()
        );
        assert_eq!(hits[0].id, "text-1");
    }

    #[test]
    fn search_clips_metadata_match() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        insert_clip(
            &store,
            &StoredClip {
                id: "meta-1".into(),
                source: "s".into(),
                source_key: None,
                source_app_id: None,
                source_app: Some("Slack".into()),
                source_url: Some("https://cinchcli.com".into()),
                label: Some("Important".into()),
                content_type: "text".into(),
                content: Some(b"nothing here".to_vec()),
                media_path: None,
                byte_size: 12,
                created_at: 10,
                pinned: false,
                pinned_at: None,
                sync_state: SyncState::Synced,
            },
        )
        .unwrap();

        // Match app name
        let hits = search_clips(&store, "Slack", 10, None, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "meta-1");

        // Match URL
        let hits = search_clips(&store, "cinchcli", 10, None, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "meta-1");
    }

    #[test]
    fn search_clips_with_type_filter() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let make = |id: &str, ct: &str| StoredClip {
            id: id.into(),
            source: "s".into(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: ct.into(),
            content: Some(b"needle".to_vec()),
            media_path: None,
            byte_size: 6,
            created_at: 10,
            pinned: false,
            pinned_at: None,
            sync_state: SyncState::Synced,
        };
        insert_clip(&store, &make("t1", "text")).unwrap();
        insert_clip(&store, &make("c1", "code")).unwrap();
        // Image content is intentionally NOT full-text indexed (schema v7):
        // base64/pixel bytes are not meaningful text. An image is therefore
        // findable only by its metadata, so give i1 a matching label.
        let mut i1 = make("i1", "image/png");
        i1.label = Some("needle".into());
        insert_clip(&store, &i1).unwrap();

        // Search for needle in code only
        let hits = search_clips(&store, "needle", 10, Some("code"), None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "c1");

        // Search for needle in images only — matched via label, not content.
        let hits = search_clips(&store, "needle", 10, Some("image"), None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "i1");

        // An image whose ONLY "needle" is in its (binary) content must not be
        // found — that base64 noise is exactly what schema v7 stops indexing.
        let i2 = make("i2", "image/png");
        insert_clip(&store, &i2).unwrap();
        let hits = search_clips(&store, "needle", 10, Some("image"), None).unwrap();
        assert_eq!(hits.len(), 1, "image content must not be FTS-searchable");
        assert_eq!(hits[0].id, "i1");
    }

    #[test]
    fn list_clips_honors_offset() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let make = |id: &str, ts: i64| StoredClip {
            id: id.into(),
            source: "s".into(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "text".into(),
            content: Some(b"x".to_vec()),
            media_path: None,
            byte_size: 1,
            created_at: ts,
            pinned: false,
            pinned_at: None,
            sync_state: SyncState::Synced,
        };
        for (id, ts) in [("a", 10), ("b", 20), ("c", 30)] {
            insert_clip(&store, &make(id, ts)).unwrap();
        }
        // Descending order: c(30), b(20), a(10)
        let rows = list_clips(&store, None, None, Some(1), Some(1), None, false, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "b");

        let rows = list_clips(&store, None, None, Some(1), Some(2), None, false, 10).unwrap();
        assert_eq!(rows[0].id, "a");
    }

    // ── exclude_source (fleet-read scope:"fleet" predicate) ──────────────────

    #[test]
    fn list_clips_exclude_source_filters_self() {
        // Fleet-read predicate: exclude_source = Some(self) must return every
        // clip whose source is NOT self, and None must return all rows
        // regardless of source. Mirrors the §9 spec case — seed remote:hostA +
        // remote:hostB and assert the split.
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let make = |id: &str, source: &str, ts: i64| StoredClip {
            id: id.into(),
            source: source.into(),
            content_type: "text".into(),
            content: Some(b"x".to_vec()),
            byte_size: 1,
            created_at: ts,
            sync_state: SyncState::Synced,
            ..Default::default()
        };
        insert_clip(&store, &make("a1", "remote:hostA", 10)).unwrap();
        insert_clip(&store, &make("a2", "remote:hostA", 20)).unwrap();
        insert_clip(&store, &make("b1", "remote:hostB", 30)).unwrap();

        // None → all three rows, regardless of source.
        let all = list_clips(&store, None, None, None, None, None, false, 10).unwrap();
        assert_eq!(all.len(), 3);

        // exclude_source = Some(hostA) → only the hostB row.
        let not_a = list_clips(
            &store,
            None,
            Some("remote:hostA"),
            None,
            None,
            None,
            false,
            10,
        )
        .unwrap();
        assert_eq!(not_a.len(), 1);
        assert_eq!(not_a[0].id, "b1");
        assert!(not_a.iter().all(|c| c.source == "remote:hostB"));

        // exclude_source = Some(hostB) → only the two hostA rows, newest first
        // (created_at DESC ordering is preserved through the residual filter).
        let not_b = list_clips(
            &store,
            None,
            Some("remote:hostB"),
            None,
            None,
            None,
            false,
            10,
        )
        .unwrap();
        assert_eq!(not_b.len(), 2);
        assert!(not_b.iter().all(|c| c.source == "remote:hostA"));
        assert_eq!(not_b[0].id, "a2");

        // Excluding an absent source drops nothing.
        let none_excluded = list_clips(
            &store,
            None,
            Some("remote:ghost"),
            None,
            None,
            None,
            false,
            10,
        )
        .unwrap();
        assert_eq!(none_excluded.len(), 3);

        // `from` (include) and `exclude_source` are independent predicates:
        // including and excluding the same source yields an empty set.
        let contradictory = list_clips(
            &store,
            Some("remote:hostA"),
            Some("remote:hostA"),
            None,
            None,
            None,
            false,
            10,
        )
        .unwrap();
        assert!(contradictory.is_empty());
    }

    #[test]
    fn query_clips_exclude_source_filters_self() {
        // The search path (query_clips/search_clips) must honor exclude_source
        // on BOTH its branches: the FTS path (search term present) and the
        // empty-term list path.
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let make = |id: &str, source: &str, body: &str, ts: i64| StoredClip {
            id: id.into(),
            source: source.into(),
            content_type: "text".into(),
            content: Some(body.as_bytes().to_vec()),
            byte_size: body.len() as i64,
            created_at: ts,
            sync_state: SyncState::Synced,
            ..Default::default()
        };
        insert_clip(&store, &make("a1", "remote:hostA", "needle alpha", 10)).unwrap();
        insert_clip(&store, &make("b1", "remote:hostB", "needle beta", 20)).unwrap();

        // FTS path: search term present, exclude self → only the other host.
        let fleet = query_clips(&store, "needle", 10, Some("remote:hostA")).unwrap();
        assert_eq!(fleet.len(), 1);
        assert_eq!(fleet[0].id, "b1");

        // FTS path, no exclude → both hosts.
        let all_fts = query_clips(&store, "needle", 10, None).unwrap();
        assert_eq!(all_fts.len(), 2);

        // Empty-term (list) path of query_clips also honors exclude_source.
        let fleet_list = query_clips(&store, "", 10, Some("remote:hostB")).unwrap();
        assert_eq!(fleet_list.len(), 1);
        assert_eq!(fleet_list[0].id, "a1");

        // search_clips threads exclude_source straight through to query_clips.
        let via_search = search_clips(&store, "needle", 10, None, Some("remote:hostA")).unwrap();
        assert_eq!(via_search.len(), 1);
        assert_eq!(via_search[0].id, "b1");
    }

    // ── purge_clips_before / count_clips_before ──────────────────────────────

    #[test]
    fn purge_clips_before_deletes_old_non_pinned_only() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let make = |id: &str, ts_ms: i64, pinned: bool| StoredClip {
            id: id.into(),
            source: "s".into(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "text".into(),
            content: Some(b"x".to_vec()),
            media_path: None,
            byte_size: 1,
            created_at: ts_ms,
            pinned,
            pinned_at: None,
            sync_state: SyncState::Synced,
        };

        // cutoff_secs = 1_000_000  →  cutoff_ms = 1_000_000_000
        let cutoff_secs: i64 = 1_000_000;
        let cutoff_ms = cutoff_secs * 1000;

        // OLD non-pinned:  created_at = cutoff_ms - 1  →  should be purged
        insert_clip(&store, &make("old-clip", cutoff_ms - 1, false)).unwrap();
        // NEW non-pinned:  created_at = cutoff_ms + 1  →  must survive
        insert_clip(&store, &make("new-clip", cutoff_ms + 1, false)).unwrap();
        // PINNED old:      created_at = cutoff_ms - 1  →  must survive (pinned-exempt)
        insert_clip(&store, &make("pinned-old", cutoff_ms - 1, true)).unwrap();
        // BOUNDARY:        created_at = cutoff_ms exactly  →  strict `<`, must survive
        insert_clip(&store, &make("boundary-clip", cutoff_ms, false)).unwrap();

        let deleted = purge_clips_before(&store, cutoff_secs).unwrap();
        assert_eq!(deleted, 1, "only the old non-pinned clip should be deleted");

        assert!(
            get_clip(&store, "old-clip").unwrap().is_none(),
            "old non-pinned clip must be gone after purge"
        );
        assert!(
            get_clip(&store, "new-clip").unwrap().is_some(),
            "new non-pinned clip must survive purge"
        );
        assert!(
            get_clip(&store, "pinned-old").unwrap().is_some(),
            "pinned clip must survive purge even if older than cutoff"
        );
        assert!(
            get_clip(&store, "boundary-clip").unwrap().is_some(),
            "boundary clip (created_at == cutoff_ms) must survive strict-< purge"
        );
    }

    #[test]
    fn count_clips_before_counts_old_non_pinned_only() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let make = |id: &str, ts_ms: i64, pinned: bool| StoredClip {
            id: id.into(),
            source: "s".into(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "text".into(),
            content: Some(b"x".to_vec()),
            media_path: None,
            byte_size: 1,
            created_at: ts_ms,
            pinned,
            pinned_at: None,
            sync_state: SyncState::Synced,
        };

        // cutoff_secs = 1_000_000  →  cutoff_ms = 1_000_000_000
        let cutoff_secs: i64 = 1_000_000;
        let cutoff_ms = cutoff_secs * 1000;

        // OLD non-pinned:  created_at = cutoff_ms - 1  →  counted
        insert_clip(&store, &make("old-clip", cutoff_ms - 1, false)).unwrap();
        // NEW non-pinned:  created_at = cutoff_ms + 1  →  not counted
        insert_clip(&store, &make("new-clip", cutoff_ms + 1, false)).unwrap();
        // PINNED old:      created_at = cutoff_ms - 1  →  NOT counted (pinned-exempt)
        insert_clip(&store, &make("pinned-old", cutoff_ms - 1, true)).unwrap();
        // BOUNDARY:        created_at = cutoff_ms exactly  →  strict `<`, not counted
        insert_clip(&store, &make("boundary-clip", cutoff_ms, false)).unwrap();

        let count = count_clips_before(&store, cutoff_secs).unwrap();
        assert_eq!(
            count, 1,
            "only the old non-pinned clip should be counted; \
             pinned-old is retention-exempt and boundary-clip is outside strict-< window"
        );
    }
}
