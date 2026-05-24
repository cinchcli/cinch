use super::models::{AlertPref, RetentionPref, SourceRow, StoredClip, StoredDevice};
use super::{Store, StoreError};
use rusqlite::params;
use rusqlite::OptionalExtension;

pub fn insert_clip(store: &Store, c: &StoredClip) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute(
            r#"INSERT OR REPLACE INTO clips
               (id, source, source_key, content_type, content, media_path, byte_size, created_at, pinned, pinned_at, sync_state)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"#,
            params![
                c.id, c.source, c.source_key, c.content_type, c.content,
                c.media_path, c.byte_size, c.created_at,
                if c.pinned { 1i64 } else { 0 }, c.pinned_at,
                c.sync_state.as_str(),
            ],
        )?;
        Ok(())
    })
}

pub fn list_clips(
    store: &Store,
    from: Option<&str>,
    limit: Option<i64>,
    since_ms: Option<i64>,
    pinned_only: bool,
    default_limit: i64,
) -> Result<Vec<StoredClip>, StoreError> {
    let mut sql = String::from(
        "SELECT id, source, source_key, content_type, content, media_path, byte_size, created_at, pinned, pinned_at, sync_state
         FROM clips WHERE 1=1"
    );
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = from {
        sql.push_str(" AND source = ?");
        binds.push(Box::new(s.to_string()));
    }
    if let Some(t) = since_ms {
        sql.push_str(" AND created_at >= ?");
        binds.push(Box::new(t));
    }
    if pinned_only {
        sql.push_str(" AND pinned = 1");
    }
    sql.push_str(" ORDER BY created_at DESC LIMIT ?");
    binds.push(Box::new(limit.unwrap_or(default_limit)));

    store.with_conn(|conn| {
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<StoredClip> = stmt
            .query_map(
                rusqlite::params_from_iter(binds.iter().map(|b| &**b as &dyn rusqlite::ToSql)),
                |r| {
                    Ok(StoredClip {
                        id: r.get(0)?,
                        source: r.get(1)?,
                        source_key: r.get(2)?,
                        content_type: r.get(3)?,
                        content: r.get(4)?,
                        media_path: r.get(5)?,
                        byte_size: r.get(6)?,
                        created_at: r.get(7)?,
                        pinned: r.get::<_, i64>(8)? != 0,
                        pinned_at: r.get(9)?,
                        sync_state: super::models::SyncState::from_str_lossy(
                            &r.get::<_, String>(10)?,
                        ),
                    })
                },
            )?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

pub fn get_clip(store: &Store, id: &str) -> Result<Option<StoredClip>, StoreError> {
    store.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, source, source_key, content_type, content, media_path, byte_size, created_at, pinned, pinned_at, sync_state
             FROM clips WHERE id = ?1"
        )?;
        let mut rows = stmt.query_map(params![id], |r| Ok(StoredClip {
            id: r.get(0)?, source: r.get(1)?, source_key: r.get(2)?,
            content_type: r.get(3)?, content: r.get(4)?, media_path: r.get(5)?,
            byte_size: r.get(6)?, created_at: r.get(7)?,
            pinned: r.get::<_, i64>(8)? != 0, pinned_at: r.get(9)?,
            sync_state: super::models::SyncState::from_str_lossy(&r.get::<_, String>(10)?),
        }))?;
        if let Some(row) = rows.next() { Ok(Some(row?)) } else { Ok(None) }
    })
}

pub fn delete_clip(store: &Store, id: &str) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute("DELETE FROM clips WHERE id = ?1", params![id])?;
        Ok(())
    })
}

pub fn set_pinned(store: &Store, id: &str, pinned: bool, when_ms: i64) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute(
            "UPDATE clips SET pinned = ?1, pinned_at = CASE WHEN ?1 = 1 THEN ?2 ELSE NULL END WHERE id = ?3",
            params![if pinned { 1i64 } else { 0 }, when_ms, id],
        )?;
        Ok(())
    })
}

pub fn list_sources(store: &Store) -> Result<Vec<SourceRow>, StoreError> {
    store.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT source, COUNT(*) AS c, MAX(created_at) AS last_seen
             FROM clips GROUP BY source ORDER BY last_seen DESC NULLS LAST",
        )?;
        let rows: Vec<SourceRow> = stmt
            .query_map([], |r| {
                Ok(SourceRow {
                    source: r.get(0)?,
                    clip_count: r.get(1)?,
                    last_seen: r.get(2)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

pub fn upsert_device(store: &Store, d: &StoredDevice) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute(
            r#"INSERT OR REPLACE INTO devices
               (id, hostname, nickname, source_key, machine_id, public_key,
                paired_at, last_push_at, online, refreshed_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
            params![
                d.id,
                d.hostname,
                d.nickname,
                d.source_key,
                d.machine_id,
                d.public_key,
                d.paired_at,
                d.last_push_at,
                if d.online { 1i64 } else { 0 },
                d.refreshed_at,
            ],
        )?;
        Ok(())
    })
}

pub fn list_devices(store: &Store) -> Result<Vec<StoredDevice>, StoreError> {
    store.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, hostname, nickname, source_key, machine_id, public_key,
                    paired_at, last_push_at, online, refreshed_at
             FROM devices ORDER BY last_push_at DESC NULLS LAST",
        )?;
        let rows: Vec<StoredDevice> = stmt
            .query_map([], |r| {
                Ok(StoredDevice {
                    id: r.get(0)?,
                    hostname: r.get(1)?,
                    nickname: r.get(2)?,
                    source_key: r.get(3)?,
                    machine_id: r.get(4)?,
                    public_key: r.get(5)?,
                    paired_at: r.get(6)?,
                    last_push_at: r.get(7)?,
                    online: r.get::<_, i64>(8)? != 0,
                    refreshed_at: r.get(9)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

pub fn set_retention(store: &Store, device_id: &str, days: i64) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute(
            "INSERT INTO retention_prefs(device_id, days) VALUES(?1, ?2)
             ON CONFLICT(device_id) DO UPDATE SET days = excluded.days",
            params![device_id, days],
        )?;
        Ok(())
    })
}

pub fn list_retention(store: &Store) -> Result<Vec<RetentionPref>, StoreError> {
    store.with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT device_id, days FROM retention_prefs")?;
        let rows: Vec<RetentionPref> = stmt
            .query_map([], |r| {
                Ok(RetentionPref {
                    device_id: r.get(0)?,
                    days: r.get(1)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

pub fn set_alert_pref(store: &Store, source: &str, enabled: bool) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute(
            "INSERT INTO alert_prefs(source, enabled) VALUES(?1, ?2)
             ON CONFLICT(source) DO UPDATE SET enabled = excluded.enabled",
            params![source, if enabled { 1i64 } else { 0 }],
        )?;
        Ok(())
    })
}

pub fn list_alert_prefs(store: &Store) -> Result<Vec<AlertPref>, StoreError> {
    store.with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT source, enabled FROM alert_prefs")?;
        let rows: Vec<AlertPref> = stmt
            .query_map([], |r| {
                Ok(AlertPref {
                    source: r.get(0)?,
                    enabled: r.get::<_, i64>(1)? != 0,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

pub fn watermark(store: &Store) -> Result<Option<String>, StoreError> {
    store.with_conn(|conn| {
        conn.query_row(
            "SELECT value FROM meta WHERE key='last_sync_watermark'",
            [],
            |r| r.get::<_, String>(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
    })
}

pub fn set_watermark(store: &Store, ulid: &str) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute(
            "INSERT INTO meta(key, value) VALUES('last_sync_watermark', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![ulid],
        )?;
        Ok(())
    })
}

/// Return the total number of clips in the store.
pub fn clip_count(store: &Store) -> Result<i64, StoreError> {
    store.with_conn(|conn| conn.query_row("SELECT COUNT(*) FROM clips", [], |r| r.get::<_, i64>(0)))
}

/// Return the number of clips whose `created_at` (ms) is before `cutoff_ms`.
/// Used by the Settings pane "preview retention change" dialog.
pub fn count_clips_before(store: &Store, cutoff_ms: i64) -> Result<i64, StoreError> {
    store.with_conn(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM clips WHERE created_at < ?1",
            rusqlite::params![cutoff_ms],
            |r| r.get::<_, i64>(0),
        )
    })
}

/// Delete all clips from the store. Returns the number of rows deleted.
pub fn clear_all_clips(store: &Store) -> Result<i64, StoreError> {
    store.with_conn(|conn| {
        let n = conn.execute("DELETE FROM clips", [])?;
        Ok(n as i64)
    })
}

pub fn search_clips(store: &Store, query: &str, limit: i64) -> Result<Vec<StoredClip>, StoreError> {
    store.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT c.id, c.source, c.source_key, c.content_type, c.content, c.media_path,
                    c.byte_size, c.created_at, c.pinned, c.pinned_at, c.sync_state
             FROM clips c JOIN clips_fts f ON f.rowid = c.rowid
             WHERE clips_fts MATCH ?1 ORDER BY rank LIMIT ?2",
        )?;
        let rows: Vec<StoredClip> = stmt
            .query_map(params![query, limit], |r| {
                Ok(StoredClip {
                    id: r.get(0)?,
                    source: r.get(1)?,
                    source_key: r.get(2)?,
                    content_type: r.get(3)?,
                    content: r.get(4)?,
                    media_path: r.get(5)?,
                    byte_size: r.get(6)?,
                    created_at: r.get(7)?,
                    pinned: r.get::<_, i64>(8)? != 0,
                    pinned_at: r.get(9)?,
                    sync_state: super::models::SyncState::from_str_lossy(&r.get::<_, String>(10)?),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

/// Return all clips that are queued for an explicit send (`sync_state = 'pending'`),
/// ordered oldest first.
pub fn list_pending_clips(store: &Store) -> Result<Vec<StoredClip>, StoreError> {
    store.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, source, source_key, content_type, content, media_path, byte_size,
                    created_at, pinned, pinned_at, sync_state
             FROM clips
             WHERE sync_state = 'pending'
             ORDER BY created_at ASC",
        )?;
        let rows: Vec<StoredClip> = stmt
            .query_map([], |r| {
                Ok(StoredClip {
                    id: r.get(0)?,
                    source: r.get(1)?,
                    source_key: r.get(2)?,
                    content_type: r.get(3)?,
                    content: r.get(4)?,
                    media_path: r.get(5)?,
                    byte_size: r.get(6)?,
                    created_at: r.get(7)?,
                    pinned: r.get::<_, i64>(8)? != 0,
                    pinned_at: r.get(9)?,
                    sync_state: super::models::SyncState::from_str_lossy(&r.get::<_, String>(10)?),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

/// Transition a clip to `Pending` (queued for an explicit send).
pub fn mark_pending(store: &Store, id: &str) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute(
            "UPDATE clips SET sync_state = 'pending' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    })
}

/// Transition a clip back to `Local` (e.g. an explicit send hit a permanent
/// error and must not be retried).
pub fn mark_local(store: &Store, id: &str) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute(
            "UPDATE clips SET sync_state = 'local' WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    })
}

/// Drop the oldest unsynced clips that exceed `max`, keeping the newest `max`.
/// Returns the number of rows deleted.
pub fn enforce_offline_cap(store: &Store, max: usize) -> Result<usize, StoreError> {
    store.with_conn(|conn| {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM clips WHERE sync_state = 'pending'",
            [],
            |r| r.get(0),
        )?;
        if (count as usize) <= max {
            return Ok(0);
        }
        let excess = count as usize - max;
        let dropped = conn.execute(
            "DELETE FROM clips
             WHERE id IN (
                 SELECT id FROM clips WHERE sync_state = 'pending' ORDER BY created_at ASC LIMIT ?1
             )",
            params![excess as i64],
        )?;
        log::warn!(
            "offline queue cap: dropped {} oldest unsynced clips (cap={})",
            dropped,
            max
        );
        Ok(dropped)
    })
}
/// Rename a clip's id (e.g. temp local id → relay-assigned ULID) and mark it synced.
/// Returns the number of rows updated (0 if the old id was not found).
pub fn replace_id_and_mark_synced(
    store: &Store,
    old_id: &str,
    new_id: &str,
) -> Result<usize, StoreError> {
    store.with_conn(|conn| {
        let n = conn.execute(
            "UPDATE clips SET id = ?1, sync_state = 'synced' WHERE id = ?2",
            params![new_id, old_id],
        )?;
        Ok(n)
    })
}
const LAST_FLUSH_KEY: &str = "backlog.last_flush_at";

/// Read the epoch-millisecond timestamp of the last successful backlog flush.
/// Returns `None` if no flush has ever been recorded.
pub fn get_last_flush_at(store: &Store) -> Result<Option<i64>, StoreError> {
    store.with_conn(|conn| {
        let v: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                params![LAST_FLUSH_KEY],
                |r| r.get(0),
            )
            .optional()?;
        Ok(v.and_then(|s| s.parse().ok()))
    })
}

/// Persist the epoch-millisecond timestamp of a successful backlog flush.
pub fn set_last_flush_at(store: &Store, ts: i64) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute(
            "INSERT INTO meta(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![LAST_FLUSH_KEY, ts.to_string()],
        )?;
        Ok(())
    })
}
#[cfg(test)]
mod tests {
    use super::super::models::{StoredClip, SyncState};
    use super::*;

    #[test]
    fn insert_clip_persists_sync_state() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let clip = StoredClip {
            id: "01HXABC".into(),
            source: "atlas0".into(),
            source_key: None,
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

    // ── Task 8: get_last_flush_at / set_last_flush_at ───────────────────────

    #[test]
    fn last_flush_at_roundtrips() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        assert!(get_last_flush_at(&store).unwrap().is_none());
        set_last_flush_at(&store, 1_700_000_000).unwrap();
        assert_eq!(get_last_flush_at(&store).unwrap(), Some(1_700_000_000));
        set_last_flush_at(&store, 1_700_001_000).unwrap();
        assert_eq!(get_last_flush_at(&store).unwrap(), Some(1_700_001_000));
    }
}
