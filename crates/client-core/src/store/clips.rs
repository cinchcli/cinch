//! Core clip CRUD: insert, fetch, delete, pin, count, and retention purge.

use super::models::StoredClip;
use super::{Store, StoreError};
use rusqlite::params;
use rusqlite::OptionalExtension;
use rusqlite::Row;

/// The `clips` columns in the exact positional order `stored_clip_from_row`
/// reads them (index 0..=14). Every `SELECT` that feeds `stored_clip_from_row`
/// must use this list, so the column order and the row decoder can never drift
/// apart (a silent off-by-one is exactly the kind of bug this prevents).
pub(super) const CLIP_COLUMNS: &str = "id, source, source_key, source_app_id, source_app, source_url, label, content_type, content, media_path, byte_size, created_at, pinned, pinned_at, sync_state";

pub(super) fn stored_clip_from_row(r: &Row<'_>) -> rusqlite::Result<StoredClip> {
    Ok(StoredClip {
        id: r.get(0)?,
        source: r.get(1)?,
        source_key: r.get(2)?,
        source_app_id: r.get(3)?,
        source_app: r.get(4)?,
        source_url: r.get(5)?,
        label: r.get(6)?,
        content_type: r.get(7)?,
        content: r.get(8)?,
        media_path: r.get(9)?,
        byte_size: r.get(10)?,
        created_at: r.get(11)?,
        pinned: r.get::<_, i64>(12)? != 0,
        pinned_at: r.get(13)?,
        sync_state: super::models::SyncState::from_str_lossy(&r.get::<_, String>(14)?),
    })
}

pub fn insert_clip(store: &Store, c: &StoredClip) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute(
            r#"INSERT OR REPLACE INTO clips
               (id, source, source_key, source_app_id, source_app, source_url, label, content_type, content, media_path, byte_size, created_at, pinned, pinned_at, sync_state)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)"#,
            params![
                c.id,
                c.source,
                c.source_key,
                c.source_app_id,
                c.source_app,
                c.source_url,
                c.label,
                c.content_type,
                c.content,
                c.media_path,
                c.byte_size,
                c.created_at,
                if c.pinned { 1i64 } else { 0 },
                c.pinned_at,
                c.sync_state.as_str(),
            ],
        )?;
        Ok(())
    })
}

/// Returns the id of an existing clip whose `content` is byte-identical to
/// `content` and that was created at or after `since_ms`, if any.
///
/// This backs the clipboard monitor's cross-process echo guard: when cinch
/// itself saves a clip and also writes it to the system clipboard (e.g.
/// `cinch session copy` saves a clip, then copies the same markdown), the
/// monitor would otherwise observe that clipboard write and re-capture it as a
/// duplicate. The CLI and desktop share one store, so the just-saved clip is
/// already visible here. The short recency window keeps the guard from
/// collapsing genuinely-repeated copies made far apart in time.
pub fn recent_clip_id_by_content(
    store: &Store,
    content: &[u8],
    since_ms: i64,
) -> Result<Option<String>, StoreError> {
    store.with_conn(|conn| {
        let id = conn
            .query_row(
                "SELECT id FROM clips WHERE content = ?1 AND created_at >= ?2 \
                 ORDER BY created_at DESC LIMIT 1",
                params![content, since_ms],
                |r| r.get::<_, String>(0),
            )
            .optional()?;
        Ok(id)
    })
}

/// List clips, newest first.
///
/// `from` is an exact-INCLUDE source filter (`source = ?`); `exclude_source`
/// is its exact-EXCLUDE counterpart (`source != ?`), used to serve the MCP
/// `scope:"fleet"` read — "every clip whose source is not this machine".
/// The two are independent: passing both yields `source = a AND source != b`.
///
/// Note: `source != ?` is a **residual filter**, not an index seek. The
/// `clips_source_idx (source, created_at DESC)` index cannot range-seek on an
/// inequality, so it serves only the `created_at DESC` ordering while the
/// excluded rows are scanned and filtered out. This is negligible at
/// single-machine local-history scale, so no new index is warranted.
#[allow(clippy::too_many_arguments)]
pub fn list_clips(
    store: &Store,
    from: Option<&str>,
    exclude_source: Option<&str>,
    limit: Option<i64>,
    offset: Option<i64>,
    since_ms: Option<i64>,
    pinned_only: bool,
    default_limit: i64,
) -> Result<Vec<StoredClip>, StoreError> {
    let mut sql = format!("SELECT {CLIP_COLUMNS} FROM clips WHERE 1=1");
    let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = from {
        sql.push_str(" AND source = ?");
        binds.push(Box::new(s.to_string()));
    }
    if let Some(s) = exclude_source {
        sql.push_str(" AND source != ?");
        binds.push(Box::new(s.to_string()));
    }
    if let Some(t) = since_ms {
        sql.push_str(" AND created_at >= ?");
        binds.push(Box::new(t));
    }
    if pinned_only {
        sql.push_str(" AND pinned = 1");
    }
    sql.push_str(" ORDER BY created_at DESC LIMIT ? OFFSET ?");
    binds.push(Box::new(limit.unwrap_or(default_limit)));
    binds.push(Box::new(offset.unwrap_or(0)));

    store.with_conn(|conn| {
        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<StoredClip> = stmt
            .query_map(
                rusqlite::params_from_iter(binds.iter().map(|b| &**b as &dyn rusqlite::ToSql)),
                stored_clip_from_row,
            )?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

pub fn get_clip(store: &Store, id: &str) -> Result<Option<StoredClip>, StoreError> {
    store.with_conn(|conn| {
        let mut stmt = conn.prepare(&format!("SELECT {CLIP_COLUMNS} FROM clips WHERE id = ?1"))?;
        let mut rows = stmt.query_map(params![id], stored_clip_from_row)?;
        if let Some(row) = rows.next() {
            Ok(Some(row?))
        } else {
            Ok(None)
        }
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

/// Return the total number of clips in the store.
pub fn clip_count(store: &Store) -> Result<i64, StoreError> {
    store.with_conn(|conn| conn.query_row("SELECT COUNT(*) FROM clips", [], |r| r.get::<_, i64>(0)))
}

/// Delete all clips from the store. Returns the number of rows deleted.
pub fn clear_all_clips(store: &Store) -> Result<i64, StoreError> {
    store.with_conn(|conn| {
        let n = conn.execute("DELETE FROM clips", [])?;
        Ok(n as i64)
    })
}

/// Delete all non-pinned clips with `created_at < cutoff_secs` (Unix seconds).
/// Returns the number of rows deleted. Pinned clips are always exempt.
pub fn purge_clips_before(store: &Store, cutoff_secs: i64) -> Result<usize, StoreError> {
    store.with_conn(|conn| {
        let n = conn.execute(
            "DELETE FROM clips WHERE created_at < ?1 AND pinned = 0",
            rusqlite::params![cutoff_secs * 1000],
        )?;
        Ok(n)
    })
}

/// Count non-pinned clips with `created_at < cutoff_secs` (Unix seconds).
/// Pinned clips are excluded — they are retention-exempt, so this count
/// reflects exactly what `purge_clips_before` would delete. (The legacy
/// desktop counter included pinned clips; this count is intentionally the
/// purge-accurate number shown in the retroactive-purge confirmation dialog.)
/// Used to populate the retroactive-purge confirmation dialog.
pub fn count_clips_before(store: &Store, cutoff_secs: i64) -> Result<i64, StoreError> {
    store.with_conn(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM clips WHERE created_at < ?1 AND pinned = 0",
            rusqlite::params![cutoff_secs * 1000],
            |r| r.get::<_, i64>(0),
        )
    })
}
