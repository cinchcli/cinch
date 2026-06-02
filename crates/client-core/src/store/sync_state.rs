//! Sync-state transitions and the offline send queue: watermark tracking,
//! pending-clip listing, and the local→synced id reconciliation.

use super::clips::stored_clip_from_row;
use super::models::StoredClip;
use super::{Store, StoreError};
use rusqlite::params;
use rusqlite::OptionalExtension;

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

/// Return all clips that are queued for an explicit send (`sync_state = 'pending'`),
/// ordered oldest first.
pub fn list_pending_clips(store: &Store) -> Result<Vec<StoredClip>, StoreError> {
    store.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, source, source_key, source_app_id, source_app, source_url, label, content_type, content, media_path, byte_size,
                    created_at, pinned, pinned_at, sync_state
             FROM clips
             WHERE sync_state = 'pending'
             ORDER BY created_at ASC",
        )?;
        let rows: Vec<StoredClip> = stmt
            .query_map([], stored_clip_from_row)?
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
///
/// Note: the relay-assigned id may already exist locally (e.g. inserted by the
/// writer from a WS event while a pending/local row is being flushed). In that
/// case we merge pin metadata into the target row and delete the old row.
pub fn replace_id_and_mark_synced(
    store: &Store,
    old_id: &str,
    new_id: &str,
) -> Result<usize, StoreError> {
    store.with_conn(|conn| {
        conn.execute_batch("BEGIN IMMEDIATE")?;

        let res: Result<usize, rusqlite::Error> = (|| {
            let old: Option<(i64, Option<i64>)> = conn
                .query_row(
                    "SELECT pinned, pinned_at FROM clips WHERE id = ?1",
                    params![old_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;

            let Some((old_pinned, old_pinned_at)) = old else {
                return Ok(0);
            };

            if old_id == new_id {
                return conn.execute(
                    "UPDATE clips SET sync_state = 'synced' WHERE id = ?1",
                    params![old_id],
                );
            }

            let target: Option<(i64, Option<i64>)> = conn
                .query_row(
                    "SELECT pinned, pinned_at FROM clips WHERE id = ?1",
                    params![new_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;

            if let Some((target_pinned, target_pinned_at)) = target {
                let merged_pinned = (old_pinned != 0) || (target_pinned != 0);
                let merged_pinned_at = match (old_pinned_at, target_pinned_at) {
                    (Some(a), Some(b)) => Some(a.max(b)),
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                };

                conn.execute(
                    "UPDATE clips SET pinned = ?1, pinned_at = ?2, sync_state = 'synced' WHERE id = ?3",
                    params![if merged_pinned { 1i64 } else { 0 }, merged_pinned_at, new_id],
                )?;
                conn.execute("DELETE FROM clips WHERE id = ?1", params![old_id])?;
                Ok(1)
            } else {
                conn.execute(
                    "UPDATE clips SET id = ?1, sync_state = 'synced' WHERE id = ?2",
                    params![new_id, old_id],
                )
            }
        })();

        match res {
            Ok(n) => {
                conn.execute_batch("COMMIT")?;
                Ok(n)
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    })
}
