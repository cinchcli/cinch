use log::info;
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::Mutex;

use super::models::LocalClip;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &PathBuf) -> Result<Self, String> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create db dir: {}", e))?;
        }

        let conn = Connection::open(path).map_err(|e| format!("failed to open db: {}", e))?;

        // Enable WAL mode + busy timeout
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| format!("failed to set pragmas: {}", e))?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;

        info!("database opened: {}", path.display());
        Ok(db)
    }

    fn migrate(&self) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS clips (
                id           TEXT PRIMARY KEY,
                user_id      TEXT NOT NULL,
                content      TEXT NOT NULL,
                content_type TEXT DEFAULT 'text',
                source       TEXT NOT NULL,
                label        TEXT DEFAULT '',
                byte_size    INTEGER DEFAULT 0,
                created_at   INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_clips_source ON clips(source);
            CREATE INDEX IF NOT EXISTS idx_clips_created ON clips(created_at DESC);

            CREATE VIRTUAL TABLE IF NOT EXISTS clips_fts USING fts5(
                content, source, label,
                content='clips', content_rowid='rowid'
            );

            -- Drop old triggers without WHEN guard (migration from pre-Phase1)
            DROP TRIGGER IF EXISTS clips_ai;
            DROP TRIGGER IF EXISTS clips_ad;
            DROP TRIGGER IF EXISTS clips_au;

            CREATE TRIGGER clips_ai AFTER INSERT ON clips
            WHEN length(new.content) > 0
            BEGIN
                INSERT INTO clips_fts(rowid, content, source, label)
                VALUES (new.rowid, substr(new.content, 1, 10240), new.source, new.label);
            END;

            CREATE TRIGGER clips_ad AFTER DELETE ON clips
            WHEN length(old.content) > 0
            BEGIN
                INSERT INTO clips_fts(clips_fts, rowid, content, source, label)
                VALUES('delete', old.rowid, substr(old.content, 1, 10240), old.source, old.label);
            END;

            CREATE TRIGGER clips_au AFTER UPDATE ON clips
            WHEN length(old.content) > 0 OR length(new.content) > 0
            BEGIN
                INSERT INTO clips_fts(clips_fts, rowid, content, source, label)
                VALUES('delete', old.rowid, substr(old.content, 1, 10240), old.source, old.label);
                INSERT INTO clips_fts(rowid, content, source, label)
                VALUES (new.rowid, substr(new.content, 1, 10240), new.source, new.label);
            END;

            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT
            );
            ",
        )
        .map_err(|e| format!("migration failed: {}", e))?;

        // Phase 2: add media_path column if not exists
        let has_media_path: bool = conn
            .prepare("PRAGMA table_info(clips)")
            .map_err(|e| format!("pragma failed: {}", e))?
            .query_map([], |row| {
                let name: String = row.get(1)?;
                Ok(name)
            })
            .map_err(|e| format!("pragma query failed: {}", e))?
            .filter_map(|r| r.ok())
            .any(|name| name == "media_path");

        if !has_media_path {
            conn.execute_batch("ALTER TABLE clips ADD COLUMN media_path TEXT DEFAULT NULL")
                .map_err(|e| format!("migration media_path failed: {}", e))?;
        }

        // Phase 1 (D-09): drop is_pinned column — pinned-clips feature cut.
        // SQLite >= 3.35 supports native DROP COLUMN; libsqlite3-sys 0.30.1
        // bundles SQLite 3.47. `clips` has no index/FK/trigger referencing
        // is_pinned (verified: triggers above only reference content/source/label/rowid).
        let has_is_pinned: bool = conn
            .prepare("PRAGMA table_info(clips)")
            .map_err(|e| format!("pragma failed: {}", e))?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("pragma query failed: {}", e))?
            .filter_map(|r| r.ok())
            .any(|name| name == "is_pinned");

        if has_is_pinned {
            conn.execute_batch("ALTER TABLE clips DROP COLUMN is_pinned")
                .map_err(|e| format!("migration drop is_pinned failed: {}", e))?;
            info!("migration: dropped is_pinned column");
        }

        // Phase 4 (D-09): add synced column for offline push queue
        let has_synced: bool = conn
            .prepare("PRAGMA table_info(clips)")
            .map_err(|e| format!("pragma synced check failed: {}", e))?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("pragma synced query failed: {}", e))?
            .filter_map(|r| r.ok())
            .any(|name| name == "synced");

        if !has_synced {
            conn.execute_batch("ALTER TABLE clips ADD COLUMN synced BOOLEAN DEFAULT TRUE")
                .map_err(|e| format!("migration synced failed: {}", e))?;
        }

        // Pin feature: add is_pinned and pin_note columns
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(clips)")
            .map_err(|e| format!("pragma pin check failed: {}", e))?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("pragma pin query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        if !cols.iter().any(|c| c == "is_pinned") {
            conn.execute_batch("ALTER TABLE clips ADD COLUMN is_pinned INTEGER NOT NULL DEFAULT 0")
                .map_err(|e| format!("migration is_pinned failed: {}", e))?;
        }
        if !cols.iter().any(|c| c == "pin_note") {
            conn.execute_batch("ALTER TABLE clips ADD COLUMN pin_note TEXT DEFAULT NULL")
                .map_err(|e| format!("migration pin_note failed: {}", e))?;
        }

        // Migrate: add received_at for delta-sync watermark.
        // Check if column already exists to avoid running the backfill UPDATE
        // on every app launch. On legacy seed schemas used in tests the FTS5
        // table may not yet be present when the UPDATE fires its trigger, so
        // we swallow the error. On production databases the FTS5 table is always
        // present because it was created earlier in this same migrate() call.
        let has_received_at: bool = conn
            .prepare("PRAGMA table_info(clips)")
            .map_err(|e| format!("pragma failed: {}", e))?
            .query_map([], |row| {
                let name: String = row.get(1)?;
                Ok(name)
            })
            .map_err(|e| format!("pragma query failed: {}", e))?
            .filter_map(|r| r.ok())
            .any(|name| name == "received_at");

        if !has_received_at {
            conn.execute(
                "ALTER TABLE clips ADD COLUMN received_at INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .map_err(|e| format!("migration received_at failed: {}", e))?;
            // On legacy seed schemas used in tests the FTS5 table may not yet
            // be present when the UPDATE fires its trigger, so we swallow the error.
            // On production databases the FTS5 table is always present because it was
            // created earlier in this same migrate() call.
            let _ = conn.execute(
                "UPDATE clips SET received_at = created_at WHERE received_at = 0",
                [],
            );
        }

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_clips_received ON clips(received_at DESC)",
            [],
        )
        .map_err(|e| format!("create idx_clips_received: {}", e))?;

        // Drop ttl column — field retired from proto; replaced by local_retention_days sweep.
        let has_ttl = conn
            .prepare("PRAGMA table_info(clips)")
            .map_err(|e| format!("pragma ttl check failed: {}", e))?
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(|e| format!("pragma ttl query failed: {}", e))?
            .any(|r| r.map(|n| n == "ttl").unwrap_or(false));
        if has_ttl {
            conn.execute_batch("ALTER TABLE clips DROP COLUMN ttl;")
                .map_err(|e| format!("migration drop ttl failed: {}", e))?;
            info!("migration: dropped ttl column");
        }

        info!("database migration complete");
        Ok(())
    }

    // Legacy clip-row writers — production callers were removed when the
    // clipboard monitor migrated to client_core::sync::LocalPusher. Kept for
    // the in-file test suite that still exercises the legacy schema, and as a
    // safety net for any future one-shot migration code.
    #[allow(dead_code)]
    pub fn insert_clip(&self, clip: &LocalClip) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO clips (id, user_id, content, content_type, source, label, byte_size, media_path, created_at, synced, is_pinned, pin_note, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET
                 content      = excluded.content,
                 content_type = excluded.content_type,
                 source       = excluded.source,
                 label        = excluded.label,
                 byte_size    = excluded.byte_size,
                 created_at   = excluded.created_at,
                 media_path   = excluded.media_path,
                 received_at  = excluded.received_at",
            params![
                clip.id,
                clip.user_id,
                clip.content,
                clip.content_type,
                clip.source,
                clip.label,
                clip.byte_size,
                clip.media_path,
                clip.created_at,
                clip.synced,
                clip.is_pinned as i32,
                clip.pin_note,
                clip.received_at,
            ],
        )
        .map_err(|e| format!("insert failed: {}", e))?;
        Ok(())
    }

    #[cfg(test)]
    pub fn list_clips(
        &self,
        source_filter: Option<&str>,
        type_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<LocalClip>, String> {
        let conn = self.conn.lock().unwrap();

        let mut sql = String::from(
            "SELECT id, user_id, content, content_type, source, label, byte_size, media_path, created_at, synced, is_pinned, pin_note, received_at
             FROM clips WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(source) = source_filter {
            sql.push_str(" AND source = ?");
            param_values.push(Box::new(source.to_string()));
        }
        if let Some(ctype) = type_filter {
            sql.push_str(" AND content_type = ?");
            param_values.push(Box::new(ctype.to_string()));
        }

        sql.push_str(" ORDER BY received_at DESC, created_at DESC LIMIT ?");
        param_values.push(Box::new(limit));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("prepare failed: {}", e))?;

        let clips = stmt
            .query_map(params_refs.as_slice(), |row| {
                Ok(LocalClip {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    content: row.get(2)?,
                    content_type: row.get(3)?,
                    source: row.get(4)?,
                    label: row.get(5)?,
                    byte_size: row.get(6)?,
                    media_path: row.get(7)?,
                    created_at: row.get(8)?,
                    synced: row.get::<_, bool>(9).unwrap_or(true),
                    is_pinned: row.get::<_, i32>(10).unwrap_or(0) != 0,
                    pin_note: row.get(11)?,
                    received_at: row.get::<_, i64>(12).unwrap_or(0),
                })
            })
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(clips)
    }

    #[cfg(test)]
    pub fn list_pinned_clips(&self) -> Result<Vec<LocalClip>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, content, content_type, source, label, byte_size, media_path, created_at, synced, is_pinned, pin_note, received_at
                 FROM clips WHERE is_pinned = 1 ORDER BY created_at DESC",
            )
            .map_err(|e| format!("prepare failed: {}", e))?;

        let clips = stmt
            .query_map([], |row| {
                Ok(LocalClip {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    content: row.get(2)?,
                    content_type: row.get(3)?,
                    source: row.get(4)?,
                    label: row.get(5)?,
                    byte_size: row.get(6)?,
                    media_path: row.get(7)?,
                    created_at: row.get(8)?,
                    synced: row.get::<_, bool>(9).unwrap_or(true),
                    is_pinned: true,
                    pin_note: row.get(11)?,
                    received_at: row.get::<_, i64>(12).unwrap_or(0),
                })
            })
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(clips)
    }

    #[cfg(test)]
    pub fn pin_clip(&self, id: &str, note: Option<&str>) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clips SET is_pinned = 1, pin_note = ?2 WHERE id = ?1",
            params![id, note],
        )
        .map_err(|e| format!("pin_clip failed: {}", e))?;
        Ok(())
    }

    #[cfg(test)]
    pub fn search_clips(&self, query: &str, limit: i64) -> Result<Vec<LocalClip>, String> {
        if query.trim().is_empty() {
            return self.list_clips(None, None, limit);
        }

        let conn = self.conn.lock().unwrap();
        let like_pattern = format!("%{}%", query);
        let mut stmt = conn
            .prepare(
                "SELECT c.id, c.user_id, c.content, c.content_type, c.source, c.label, c.byte_size, c.media_path, c.created_at, c.synced, c.is_pinned, c.pin_note, c.received_at
                 FROM clips c
                 JOIN clips_fts f ON c.rowid = f.rowid
                 WHERE clips_fts MATCH ?1
                 UNION
                 SELECT c.id, c.user_id, c.content, c.content_type, c.source, c.label, c.byte_size, c.media_path, c.created_at, c.synced, c.is_pinned, c.pin_note, c.received_at
                 FROM clips c
                 WHERE c.is_pinned = 1 AND c.pin_note LIKE ?2
                 ORDER BY created_at DESC
                 LIMIT ?3",
            )
            .map_err(|e| format!("prepare failed: {}", e))?;

        let clips = stmt
            .query_map(params![query, like_pattern, limit], |row| {
                Ok(LocalClip {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    content: row.get(2)?,
                    content_type: row.get(3)?,
                    source: row.get(4)?,
                    label: row.get(5)?,
                    byte_size: row.get(6)?,
                    media_path: row.get(7)?,
                    created_at: row.get(8)?,
                    synced: row.get::<_, bool>(9).unwrap_or(true),
                    is_pinned: row.get::<_, i32>(10).unwrap_or(0) != 0,
                    pin_note: row.get(11)?,
                    received_at: row.get::<_, i64>(12).unwrap_or(0),
                })
            })
            .map_err(|e| format!("search failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(clips)
    }

    #[cfg(test)]
    pub fn get_sources(&self) -> Result<Vec<SourceInfo>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT source, COUNT(*) as count, MAX(created_at) as last_seen
                 FROM clips
                 GROUP BY source
                 ORDER BY last_seen DESC",
            )
            .map_err(|e| format!("prepare failed: {}", e))?;

        let sources = stmt
            .query_map([], |row| {
                Ok(SourceInfo {
                    source: row.get(0)?,
                    clip_count: row.get(1)?,
                    last_seen: row.get(2)?,
                })
            })
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(sources)
    }

    #[cfg(test)]
    pub fn delete_clip(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();

        // Check for media file to cascade-delete
        let media_path: Option<String> = conn
            .query_row(
                "SELECT media_path FROM clips WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .ok();

        conn.execute("DELETE FROM clips WHERE id = ?1", params![id])
            .map_err(|e| format!("delete failed: {}", e))?;

        // Delete media file if present
        if let Some(Some(mp)) = media_path.map(|p| if p.is_empty() { None } else { Some(p) }) {
            let media_dir = dirs::data_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap().join(".local/share"))
                .join("com.cinch.app");
            let full_path = media_dir.join(&mp);
            let _ = std::fs::remove_file(full_path);
        }

        Ok(())
    }

    /// Delete every clip row with `created_at < cutoff` and cascade-delete
    /// its media file. Returns the number of rows deleted.
    ///
    /// Uses rusqlite `params!` parameter binding — string formatting of the
    /// cutoff is forbidden (Tampering / SQLi; see plan 01-02 threat model).
    pub fn purge_before(&self, cutoff: i64) -> Result<usize, String> {
        let conn = self.conn.lock().unwrap();

        // 1. Collect media paths of soon-to-be-deleted rows.
        let mut stmt = conn
            .prepare(
                "SELECT media_path FROM clips WHERE created_at < ?1 AND is_pinned = 0 \
                 AND media_path IS NOT NULL AND media_path != ''",
            )
            .map_err(|e| format!("prepare failed: {}", e))?;
        let media_paths: Vec<String> = stmt
            .query_map(params![cutoff], |row| row.get(0))
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        // 2. DELETE (parameterised). Pinned clips are exempt from retention purge.
        let deleted = conn
            .execute(
                "DELETE FROM clips WHERE created_at < ?1 AND is_pinned = 0",
                params![cutoff],
            )
            .map_err(|e| format!("purge failed: {}", e))?;

        // 3. Cascade-delete media files (same idiom as cleanup_expired).
        if !media_paths.is_empty() {
            let media_dir = dirs::data_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap().join(".local/share"))
                .join("com.cinch.app");
            for mp in &media_paths {
                let _ = std::fs::remove_file(media_dir.join(mp));
            }
        }

        if deleted > 0 {
            info!(
                "retention: purged {} clips older than cutoff {}",
                deleted, cutoff
            );
        }
        Ok(deleted)
    }

    /// Count clips with `created_at < cutoff` without deleting.
    /// Used to populate the retroactive-purge confirmation dialog.
    /// Called from `commands::clips::preview_retention_change` (plan 01-06).
    pub fn count_clips_before(&self, cutoff: i64) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM clips WHERE created_at < ?1",
            params![cutoff],
            |row| row.get(0),
        )
        .map_err(|e| format!("count_clips_before failed: {}", e))
    }

    /// Delete every clip row and cascade-delete every media file.
    /// Returns the number of rows deleted as `i64`.
    /// Called from `commands::clips::clear_local_history` (plan 01-06).
    pub fn clear_all_clips(&self) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();

        // 1. Collect all media paths.
        let mut stmt = conn
            .prepare(
                "SELECT media_path FROM clips WHERE media_path IS NOT NULL AND media_path != ''",
            )
            .map_err(|e| format!("prepare failed: {}", e))?;
        let media_paths: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        // 2. Unconditional DELETE.
        let deleted = conn
            .execute("DELETE FROM clips", [])
            .map_err(|e| format!("clear failed: {}", e))? as i64;

        // 3. Cascade-delete media files.
        if !media_paths.is_empty() {
            let media_dir = dirs::data_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap().join(".local/share"))
                .join("com.cinch.app");
            for mp in &media_paths {
                let _ = std::fs::remove_file(media_dir.join(mp));
            }
        }

        if deleted > 0 {
            info!("clear_local_history: deleted {} clips", deleted);
        }
        Ok(deleted)
    }

    /// Returns all clips with `synced = false`, ordered by `created_at ASC` (oldest first).
    /// Used by the offline push queue to flush pending clips on reconnect.
    #[cfg(test)]
    pub fn list_unsynced_clips(&self) -> Result<Vec<LocalClip>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, content, content_type, source, label, byte_size, media_path, created_at, synced, is_pinned, pin_note, received_at
                 FROM clips WHERE synced = FALSE ORDER BY created_at ASC",
            )
            .map_err(|e| format!("prepare failed: {}", e))?;

        let clips = stmt
            .query_map([], |row| {
                Ok(LocalClip {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    content: row.get(2)?,
                    content_type: row.get(3)?,
                    source: row.get(4)?,
                    label: row.get(5)?,
                    byte_size: row.get(6)?,
                    media_path: row.get(7)?,
                    created_at: row.get(8)?,
                    synced: row.get::<_, bool>(9).unwrap_or(true),
                    is_pinned: row.get::<_, i32>(10).unwrap_or(0) != 0,
                    pin_note: row.get(11)?,
                    received_at: row.get::<_, i64>(12).unwrap_or(0),
                })
            })
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(clips)
    }

    /// Mark a clip as synced after successful push to relay.
    #[cfg(test)]
    pub fn mark_synced(&self, clip_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clips SET synced = TRUE WHERE id = ?1",
            params![clip_id],
        )
        .map_err(|e| format!("mark_synced failed: {}", e))?;
        Ok(())
    }

    /// Enforce the offline queue cap by dropping the oldest unsynced clips
    /// when the count exceeds `max_unsynced`. Returns the number of clips dropped.
    /// Mitigates T-04-07 (DoS via unbounded DB growth during extended offline).
    ///
    /// Currently exercised only by the in-file tests — production callers were
    /// removed when the clipboard monitor moved to `LocalPusher`. A real
    /// offline-queue replacement on the shared store is a follow-up.
    #[allow(dead_code)]
    pub fn enforce_offline_cap(&self, max_unsynced: usize) -> Result<usize, String> {
        let conn = self.conn.lock().unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clips WHERE synced = FALSE",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("count unsynced failed: {}", e))?;

        let count = count as usize;
        if count <= max_unsynced {
            return Ok(0);
        }

        let excess = count - max_unsynced;
        conn.execute(
            "DELETE FROM clips WHERE id IN (
                SELECT id FROM clips WHERE synced = FALSE
                ORDER BY created_at ASC LIMIT ?1
            )",
            params![excess as i64],
        )
        .map_err(|e| format!("enforce_offline_cap failed: {}", e))?;

        info!(
            "offline queue cap: dropped {} oldest unsynced clips (cap={})",
            excess, max_unsynced
        );
        Ok(excess)
    }

    #[cfg(test)]
    pub fn clip_count(&self) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM clips", [], |row| row.get(0))
            .map_err(|e| format!("count failed: {}", e))
    }

    // --- Settings ---

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        );
        match result {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("get_setting failed: {}", e)),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            params![key, value],
        )
        .map_err(|e| format!("set_setting failed: {}", e))?;
        Ok(())
    }

    pub fn is_source_auto_copy(&self, source: &str) -> Result<bool, String> {
        let key = format!("auto_copy:{}", source);
        match self.get_setting(&key)? {
            Some(val) => Ok(val == "true"),
            None => Ok(false),
        }
    }

    pub fn set_source_auto_copy(&self, source: &str, enabled: bool) -> Result<(), String> {
        let key = format!("auto_copy:{}", source);
        self.set_setting(&key, if enabled { "true" } else { "false" })
    }

    pub fn is_source_alert_enabled(&self, source: &str) -> Result<bool, String> {
        let key = format!("alert_enabled:{}", source);
        match self.get_setting(&key)? {
            Some(val) => Ok(val == "true"),
            None => Ok(true),
        }
    }

    pub fn set_source_alert_enabled(&self, source: &str, enabled: bool) -> Result<(), String> {
        let key = format!("alert_enabled:{}", source);
        self.set_setting(&key, if enabled { "true" } else { "false" })
    }

    /// Returns true if this source has never had an auto_copy setting saved.
    #[cfg(test)]
    pub fn is_source_new(&self, source: &str) -> Result<bool, String> {
        let key = format!("auto_copy:{}", source);
        Ok(self.get_setting(&key)?.is_none())
    }

    /// Returns auto_copy status for all known sources.
    pub fn get_all_source_settings(&self) -> Result<Vec<SourceSetting>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings WHERE key LIKE 'auto_copy:%'")
            .map_err(|e| format!("prepare failed: {}", e))?;

        let settings = stmt
            .query_map([], |row| {
                let key: String = row.get(0)?;
                let value: String = row.get(1)?;
                let source = key.strip_prefix("auto_copy:").unwrap_or(&key).to_string();
                Ok(SourceSetting {
                    source,
                    auto_copy: value == "true",
                })
            })
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(settings)
    }

    pub fn get_all_source_alert_settings(&self) -> Result<Vec<SourceAlertSetting>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT key, value FROM settings WHERE key LIKE 'alert_enabled:%'")
            .map_err(|e| format!("prepare failed: {}", e))?;

        let settings = stmt
            .query_map([], |row| {
                let key: String = row.get(0)?;
                let value: String = row.get(1)?;
                let source = key
                    .strip_prefix("alert_enabled:")
                    .unwrap_or(&key)
                    .to_string();
                Ok(SourceAlertSetting {
                    source,
                    alert_enabled: value == "true",
                })
            })
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(settings)
    }

    pub fn mark_clip_copied(&self, id: &str, copied_at: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clips SET received_at = ?2 WHERE id = ?1",
            params![id, copied_at],
        )
        .map_err(|e| format!("mark_clip_copied failed: {}", e))?;
        Ok(())
    }
}

use serde::{Deserialize, Serialize};
use specta::Type;

#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SourceInfo {
    pub source: String,
    pub clip_count: i64,
    pub last_seen: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SourceSetting {
    pub source: String,
    pub auto_copy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SourceAlertSetting {
    pub source: String,
    pub alert_enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Database {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp = std::env::temp_dir().join(format!("cinch-test-{}-{}.db", std::process::id(), n));
        // Clean up any leftover from previous runs
        let _ = std::fs::remove_file(&tmp);
        Database::open(&tmp).unwrap()
    }

    fn make_clip(id: &str, content: &str, source: &str, content_type: &str) -> LocalClip {
        LocalClip {
            id: id.to_string(),
            user_id: "user1".to_string(),
            content: content.to_string(),
            content_type: content_type.to_string(),
            source: source.to_string(),
            label: "".to_string(),
            byte_size: content.len() as i64,
            media_path: None,
            created_at: chrono::Utc::now().timestamp(),
            synced: true,
            is_pinned: false,
            pin_note: None,
            received_at: 0,
        }
    }

    #[test]
    fn test_insert_and_list() {
        let db = test_db();
        let clip = make_clip("c1", "hello world", "remote:prod", "text");
        db.insert_clip(&clip).unwrap();

        let clips = db.list_clips(None, None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].id, "c1");
        assert_eq!(clips[0].content, "hello world");
    }

    #[test]
    fn test_source_filter() {
        let db = test_db();
        db.insert_clip(&make_clip("c1", "from prod", "remote:prod", "text"))
            .unwrap();
        db.insert_clip(&make_clip("c2", "from staging", "remote:staging", "text"))
            .unwrap();

        let clips = db.list_clips(Some("remote:prod"), None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].source, "remote:prod");
    }

    #[test]
    fn test_fts_search() {
        let db = test_db();
        db.insert_clip(&make_clip(
            "c1",
            "connection refused error",
            "remote:prod",
            "error",
        ))
        .unwrap();
        db.insert_clip(&make_clip("c2", "hello world", "remote:prod", "text"))
            .unwrap();

        let results = db.search_clips("connection", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "c1");
    }

    #[test]
    fn test_get_sources() {
        let db = test_db();
        db.insert_clip(&make_clip("c1", "a", "remote:prod", "text"))
            .unwrap();
        db.insert_clip(&make_clip("c2", "b", "remote:prod", "text"))
            .unwrap();
        db.insert_clip(&make_clip("c3", "c", "remote:staging", "text"))
            .unwrap();

        let sources = db.get_sources().unwrap();
        assert_eq!(sources.len(), 2);
    }

    #[test]
    fn test_delete() {
        let db = test_db();
        db.insert_clip(&make_clip("c1", "hello", "remote:prod", "text"))
            .unwrap();
        db.delete_clip("c1").unwrap();
        let clips = db.list_clips(None, None, 50).unwrap();
        assert!(clips.is_empty());
    }

    #[test]
    fn test_settings_crud() {
        let db = test_db();

        // No setting yet
        assert_eq!(db.get_setting("foo").unwrap(), None);

        // Set and get
        db.set_setting("foo", "bar").unwrap();
        assert_eq!(db.get_setting("foo").unwrap(), Some("bar".to_string()));

        // Overwrite
        db.set_setting("foo", "baz").unwrap();
        assert_eq!(db.get_setting("foo").unwrap(), Some("baz".to_string()));
    }

    #[test]
    fn test_source_auto_copy() {
        let db = test_db();

        // New source has no setting
        assert!(db.is_source_new("remote:prod").unwrap());
        assert!(!db.is_source_auto_copy("remote:prod").unwrap());

        // Enable auto_copy
        db.set_source_auto_copy("remote:prod", true).unwrap();
        assert!(!db.is_source_new("remote:prod").unwrap());
        assert!(db.is_source_auto_copy("remote:prod").unwrap());

        // Disable auto_copy
        db.set_source_auto_copy("remote:prod", false).unwrap();
        assert!(!db.is_source_auto_copy("remote:prod").unwrap());

        // get_all_source_settings
        db.set_source_auto_copy("remote:staging", true).unwrap();
        let settings = db.get_all_source_settings().unwrap();
        assert_eq!(settings.len(), 2);
    }

    #[test]
    fn test_source_alert_enabled_defaults_on_and_can_be_disabled() {
        let db = test_db();

        assert!(db.is_source_alert_enabled("remote:prod").unwrap());

        db.set_source_alert_enabled("remote:prod", false).unwrap();
        assert!(!db.is_source_alert_enabled("remote:prod").unwrap());

        db.set_source_alert_enabled("remote:prod", true).unwrap();
        assert!(db.is_source_alert_enabled("remote:prod").unwrap());

        let settings = db.get_all_source_alert_settings().unwrap();
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0].source, "remote:prod");
        assert!(settings[0].alert_enabled);
    }

    #[test]
    fn test_mark_clip_copied_updates_received_at_for_local_recency_without_changing_created_at() {
        let db = test_db();
        let mut old = make_clip("old", "old content", "remote:prod", "text");
        old.created_at = 100;
        old.received_at = 100;
        db.insert_clip(&old).unwrap();

        let mut new = make_clip("new", "new content", "remote:prod", "text");
        new.created_at = 200;
        new.received_at = 200;
        db.insert_clip(&new).unwrap();

        db.mark_clip_copied("old", 300).unwrap();

        let clips = db.list_clips(None, None, 50).unwrap();
        assert_eq!(clips[0].id, "old");
        assert_eq!(clips[0].created_at, 100);
        assert_eq!(clips[0].received_at, 300);
    }

    #[test]
    fn test_fts5_skips_empty_content() {
        let db = test_db();
        // Insert a clip with empty content (simulates future image clip)
        let mut clip = make_clip("img1", "", "local", "image");
        clip.byte_size = 0;
        db.insert_clip(&clip).unwrap();

        // FTS5 search should return no results for empty content
        let results = db.search_clips("", 50).unwrap();
        // Empty query returns all clips via list_clips fallback
        assert_eq!(results.len(), 1);

        // Actual FTS5 search should not find the empty clip
        let results = db.search_clips("anything", 50).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_local_source_clip() {
        let db = test_db();
        let clip = make_clip("l1", "local text", "local", "text");
        db.insert_clip(&clip).unwrap();

        // Should be findable via source filter
        let clips = db.list_clips(Some("local"), None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].source, "local");

        // Should be searchable
        let results = db.search_clips("local text", 50).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_insert_image_clip_with_media_path() {
        let db = test_db();
        let mut clip = make_clip("img1", "", "local", "image");
        clip.media_path = Some("media/img1.png".to_string());
        clip.byte_size = 1024;
        db.insert_clip(&clip).unwrap();

        let clips = db.list_clips(None, Some("image"), 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].media_path, Some("media/img1.png".to_string()));
        assert_eq!(clips[0].byte_size, 1024);
        assert_eq!(clips[0].content, "");
    }

    #[test]
    fn test_search_does_not_return_image_clips() {
        let db = test_db();
        // Insert text clip
        db.insert_clip(&make_clip("t1", "searchable text", "local", "text"))
            .unwrap();
        // Insert image clip (empty content)
        let mut img = make_clip("img1", "", "local", "image");
        img.media_path = Some("media/img1.png".to_string());
        db.insert_clip(&img).unwrap();

        // Text search should only find the text clip
        let results = db.search_clips("searchable", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "t1");
    }

    #[test]
    fn test_mixed_text_and_image_clips() {
        let db = test_db();
        db.insert_clip(&make_clip("t1", "hello", "remote:prod", "text"))
            .unwrap();

        let mut img = make_clip("img1", "", "local", "image");
        img.media_path = Some("media/img1.png".to_string());
        db.insert_clip(&img).unwrap();

        db.insert_clip(&make_clip("t2", "world", "local", "text"))
            .unwrap();

        // All clips
        let all = db.list_clips(None, None, 50).unwrap();
        assert_eq!(all.len(), 3);

        // Filter by image
        let images = db.list_clips(None, Some("image"), 50).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].id, "img1");

        // Filter by text
        let texts = db.list_clips(None, Some("text"), 50).unwrap();
        assert_eq!(texts.len(), 2);
    }

    // --- Retention methods (plan 01-02) ---

    #[test]
    fn purge_before_deletes_old_rows() {
        let db = test_db();
        let now = chrono::Utc::now().timestamp();
        let thirty_days = 30 * 86_400_i64;

        // Row "old" is older than 30 days; should be purged.
        let mut old_clip = make_clip("old", "old content", "remote:prod", "text");
        old_clip.created_at = now - 100 * 86_400;
        db.insert_clip(&old_clip).unwrap();

        // Row "new" is 1 day old; should survive.
        let mut new_clip = make_clip("new", "new content", "remote:prod", "text");
        new_clip.created_at = now - 86_400;
        db.insert_clip(&new_clip).unwrap();

        let deleted = db.purge_before(now - thirty_days).unwrap();
        assert_eq!(deleted, 1, "exactly one row should be purged");

        // Verify new row survived.
        let count = db.clip_count().unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn purge_before_cascades_media() {
        let db = test_db();
        let now = chrono::Utc::now().timestamp();
        let thirty_days = 30 * 86_400_i64;

        // Text row (no media_path) older than 30 days.
        let mut text_clip = make_clip("txt-old", "text only", "local", "text");
        text_clip.created_at = now - 100 * 86_400;
        db.insert_clip(&text_clip).unwrap();

        // Image row (with media_path) older than 30 days.
        // Use a unique filename under a temp-like subdirectory so test doesn't
        // clobber a real media dir. std::fs::remove_file is best-effort (matches
        // cleanup_expired pattern), so the test passes even if the file does
        // not exist on disk.
        let media_rel = format!("cinch-test-media-{}-{}.png", std::process::id(), now);
        let mut img_clip = make_clip("img-old", "", "local", "image");
        img_clip.media_path = Some(media_rel.clone());
        img_clip.created_at = now - 100 * 86_400;
        db.insert_clip(&img_clip).unwrap();

        // Pre-create the media file so we can verify cascade removal.
        let media_dir = dirs::data_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap().join(".local/share"))
            .join("com.cinch.app");
        let _ = std::fs::create_dir_all(&media_dir);
        let full_path = media_dir.join(&media_rel);
        let _ = std::fs::write(&full_path, b"fake png bytes");
        let existed_before = full_path.exists();

        let deleted = db.purge_before(now - thirty_days).unwrap();
        assert_eq!(deleted, 2, "both rows should be purged");
        assert_eq!(db.clip_count().unwrap(), 0);

        // If we successfully created the file above, verify cascade removed it.
        if existed_before {
            assert!(
                !full_path.exists(),
                "media file should be cascade-deleted: {}",
                full_path.display()
            );
        }
        // Defensive cleanup in case the assert above was skipped.
        let _ = std::fs::remove_file(&full_path);
    }

    #[test]
    fn count_clips_before_returns_correct_count() {
        let db = test_db();
        let now = chrono::Utc::now().timestamp();

        // Three rows at 10d, 40d, 80d old.
        for (id, days_ago) in [("a", 10_i64), ("b", 40), ("c", 80)] {
            let mut c = make_clip(id, id, "remote:prod", "text");
            c.created_at = now - days_ago * 86_400;
            db.insert_clip(&c).unwrap();
        }

        let count = db.count_clips_before(now - 30 * 86_400).unwrap();
        assert_eq!(count, 2, "rows at 40d and 80d should be counted");
    }

    #[test]
    fn count_clips_before_boundary() {
        let db = test_db();
        let now = chrono::Utc::now().timestamp();
        let mut c = make_clip("boundary", "at cutoff", "remote:prod", "text");
        c.created_at = now - 30 * 86_400; // exactly at cutoff — NOT strictly less than
        db.insert_clip(&c).unwrap();

        let count = db.count_clips_before(now - 30 * 86_400).unwrap();
        assert_eq!(count, 0, "< is strict; rows AT cutoff should not count");
    }

    #[test]
    fn clear_all_clips_removes_everything() {
        let db = test_db();
        for i in 0..5 {
            let c = make_clip(&format!("id-{}", i), "content", "remote:prod", "text");
            db.insert_clip(&c).unwrap();
        }
        assert_eq!(db.clip_count().unwrap(), 5);

        let deleted = db.clear_all_clips().unwrap();
        assert_eq!(deleted, 5);
        assert_eq!(db.clip_count().unwrap(), 0);
    }

    #[test]
    fn migrate_drops_is_pinned() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp = std::env::temp_dir().join(format!("cinch-drop-{}-{}.db", std::process::id(), n));
        let _ = std::fs::remove_file(&tmp);

        // Seed legacy schema with is_pinned column + one row.
        {
            let conn = rusqlite::Connection::open(&tmp).unwrap();
            conn.execute_batch(
                "CREATE TABLE clips (
                    id TEXT PRIMARY KEY,
                    user_id TEXT NOT NULL,
                    content TEXT NOT NULL,
                    content_type TEXT DEFAULT 'text',
                    source TEXT NOT NULL,
                    label TEXT DEFAULT '',
                    byte_size INTEGER DEFAULT 0,
                    is_pinned BOOLEAN DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    ttl INTEGER DEFAULT 0
                );
                CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO clips (id, user_id, content, source, is_pinned, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params!["legacy-1", "u1", "hello", "local", 1i64, 1700000000i64],
            )
            .unwrap();
        }

        // Migration runs on Database::open.
        let db = Database::open(&tmp).unwrap();

        // is_pinned is re-added by the pin feature migration — verify it exists
        // with the correct type and that legacy data survived.
        let conn = db.conn.lock().unwrap();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(clips)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(
            cols.iter().any(|c| c == "is_pinned"),
            "is_pinned should be present after pin feature migration: {:?}",
            cols
        );
        assert!(
            cols.iter().any(|c| c == "pin_note"),
            "pin_note should be present after pin feature migration: {:?}",
            cols
        );

        // Data preserved?
        let content: String = conn
            .query_row(
                "SELECT content FROM clips WHERE id = 'legacy-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(content, "hello");

        drop(conn);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn migrate_drops_is_pinned_idempotent() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp =
            std::env::temp_dir().join(format!("cinch-drop-idem-{}-{}.db", std::process::id(), n));
        let _ = std::fs::remove_file(&tmp);

        let _db1 = Database::open(&tmp).unwrap();
        let db2 = Database::open(&tmp).unwrap(); // must not panic
        let conn = db2.conn.lock().unwrap();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(clips)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        // is_pinned is re-added by pin feature migration; second open must not panic
        assert!(cols.iter().any(|c| c == "is_pinned"));
        assert!(cols.iter().any(|c| c == "pin_note"));
        drop(conn);
        let _ = std::fs::remove_file(&tmp);
    }

    // --- Synced column tests (plan 04-03, Task 1) ---

    #[test]
    fn test_synced_column_exists_after_migration() {
        let db = test_db();
        let conn = db.conn.lock().unwrap();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(clips)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(
            cols.iter().any(|c| c == "synced"),
            "synced column should exist after migration: {:?}",
            cols
        );
    }

    #[test]
    fn test_insert_clip_synced_true() {
        let db = test_db();
        let clip = make_clip("s1", "synced content", "local", "text");
        assert!(clip.synced);
        db.insert_clip(&clip).unwrap();

        let clips = db.list_clips(None, None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert!(
            clips[0].synced,
            "clip inserted with synced=true should read back as true"
        );
    }

    #[test]
    fn test_insert_clip_synced_false() {
        let db = test_db();
        let mut clip = make_clip("s2", "unsynced content", "local", "text");
        clip.synced = false;
        db.insert_clip(&clip).unwrap();

        let clips = db.list_clips(None, None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert!(
            !clips[0].synced,
            "clip inserted with synced=false should read back as false"
        );
    }

    #[test]
    fn test_existing_clips_default_synced_true() {
        // Simulate a pre-migration database by inserting a row without the synced column,
        // then verify that reading it back defaults to synced=true
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp = std::env::temp_dir().join(format!(
            "cinch-synced-default-{}-{}.db",
            std::process::id(),
            n
        ));
        let _ = std::fs::remove_file(&tmp);

        // Create a legacy schema WITHOUT the synced column and insert a row
        {
            let conn = rusqlite::Connection::open(&tmp).unwrap();
            conn.execute_batch(
                "CREATE TABLE clips (
                    id TEXT PRIMARY KEY,
                    user_id TEXT NOT NULL,
                    content TEXT NOT NULL,
                    content_type TEXT DEFAULT 'text',
                    source TEXT NOT NULL,
                    label TEXT DEFAULT '',
                    byte_size INTEGER DEFAULT 0,
                    media_path TEXT DEFAULT NULL,
                    created_at INTEGER NOT NULL,
                    ttl INTEGER DEFAULT 0
                );
                CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT);",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO clips (id, user_id, content, source, created_at) VALUES ('legacy-synced', 'u1', 'old clip', 'local', 1700000000)",
                [],
            )
            .unwrap();
        }

        // Open via Database (triggers migration)
        let db = Database::open(&tmp).unwrap();
        let clips = db.list_clips(None, None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert!(
            clips[0].synced,
            "pre-migration clip should default to synced=true"
        );

        let _ = std::fs::remove_file(&tmp);
    }

    // --- Offline queue tests (plan 04-03, Task 2) ---

    #[test]
    fn test_list_unsynced_clips_returns_only_unsynced() {
        let db = test_db();
        let mut synced_clip = make_clip("s1", "synced", "local", "text");
        synced_clip.synced = true;
        db.insert_clip(&synced_clip).unwrap();

        let mut unsynced_clip = make_clip("u1", "unsynced", "local", "text");
        unsynced_clip.synced = false;
        unsynced_clip.created_at = chrono::Utc::now().timestamp() + 1;
        db.insert_clip(&unsynced_clip).unwrap();

        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 1);
        assert_eq!(unsynced[0].id, "u1");
    }

    #[test]
    fn test_list_unsynced_clips_ordered_by_created_at_asc() {
        let db = test_db();
        let now = chrono::Utc::now().timestamp();

        let mut clip_old = make_clip("u-old", "old", "local", "text");
        clip_old.synced = false;
        clip_old.created_at = now - 100;
        db.insert_clip(&clip_old).unwrap();

        let mut clip_new = make_clip("u-new", "new", "local", "text");
        clip_new.synced = false;
        clip_new.created_at = now;
        db.insert_clip(&clip_new).unwrap();

        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 2);
        assert_eq!(unsynced[0].id, "u-old", "oldest first");
        assert_eq!(unsynced[1].id, "u-new", "newest last");
    }

    #[test]
    fn test_list_unsynced_clips_empty_when_all_synced() {
        let db = test_db();
        db.insert_clip(&make_clip("s1", "synced", "local", "text"))
            .unwrap();

        let unsynced = db.list_unsynced_clips().unwrap();
        assert!(unsynced.is_empty());
    }

    #[test]
    fn test_mark_synced() {
        let db = test_db();
        let mut clip = make_clip("u1", "unsynced", "local", "text");
        clip.synced = false;
        db.insert_clip(&clip).unwrap();

        // Verify it starts unsynced
        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 1);

        // Mark synced
        db.mark_synced("u1").unwrap();

        // Verify it's now synced
        let unsynced = db.list_unsynced_clips().unwrap();
        assert!(unsynced.is_empty());

        let all = db.list_clips(None, None, 50).unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].synced);
    }

    #[test]
    fn test_enforce_offline_cap_drops_oldest() {
        let db = test_db();
        let now = chrono::Utc::now().timestamp();

        // Insert 5 unsynced clips
        for i in 0..5 {
            let mut clip = make_clip(
                &format!("u{}", i),
                &format!("content {}", i),
                "local",
                "text",
            );
            clip.synced = false;
            clip.created_at = now + i as i64; // ascending order
            db.insert_clip(&clip).unwrap();
        }

        // Cap at 3: should drop 2 oldest (u0, u1)
        let dropped = db.enforce_offline_cap(3).unwrap();
        assert_eq!(dropped, 2);

        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 3);
        assert_eq!(unsynced[0].id, "u2");
        assert_eq!(unsynced[1].id, "u3");
        assert_eq!(unsynced[2].id, "u4");
    }

    #[test]
    fn test_enforce_offline_cap_noop_under_cap() {
        let db = test_db();

        let mut clip = make_clip("u1", "content", "local", "text");
        clip.synced = false;
        db.insert_clip(&clip).unwrap();

        let dropped = db.enforce_offline_cap(500).unwrap();
        assert_eq!(dropped, 0);

        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 1);
    }

    #[test]
    fn test_upsert_preserves_pin() {
        let db = test_db();

        // 1. Insert a clip initially
        let mut clip = make_clip("upsert-test", "original content", "remote:prod", "text");
        clip.synced = true;
        db.insert_clip(&clip).unwrap();

        // 2. Pin it with a note
        db.pin_clip("upsert-test", Some("my important note"))
            .unwrap();

        // 3. Verify it's pinned
        let pinned = db.list_pinned_clips().unwrap();
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].is_pinned, true);
        assert_eq!(pinned[0].pin_note, Some("my important note".to_string()));

        // 4. Upsert the same clip with different content (simulating relay re-delivery)
        let mut updated_clip = make_clip("upsert-test", "updated content", "remote:prod", "text");
        updated_clip.synced = true;
        updated_clip.is_pinned = false; // incoming clip doesn't know about pin state
        updated_clip.pin_note = None; // incoming clip has no pin note
        db.insert_clip(&updated_clip).unwrap();

        // 5. Verify the pin state is STILL present (not overwritten)
        let clips = db.list_clips(None, None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].id, "upsert-test");
        assert_eq!(
            clips[0].content, "updated content",
            "mutable fields should be updated"
        );
        assert_eq!(
            clips[0].is_pinned, true,
            "is_pinned should be preserved from local state"
        );
        assert_eq!(
            clips[0].pin_note,
            Some("my important note".to_string()),
            "pin_note should be preserved from local state"
        );

        // 6. Verify pinned list still shows it
        let pinned = db.list_pinned_clips().unwrap();
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].is_pinned, true);
    }

    #[test]
    fn test_upsert_preserves_synced() {
        let db = test_db();

        // 1. Insert a clip with synced=false (offline local push)
        let mut clip = make_clip("synced-upsert", "local content", "local", "text");
        clip.synced = false;
        db.insert_clip(&clip).unwrap();

        // 2. Verify it's unsynced
        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 1);
        assert_eq!(unsynced[0].synced, false);

        // 3. Upsert with new content but incoming synced=true (relay doesn't set our synced flag)
        let mut relay_clip =
            make_clip("synced-upsert", "updated from relay", "remote:prod", "text");
        relay_clip.synced = true; // relay clip always has synced=true
        db.insert_clip(&relay_clip).unwrap();

        // 4. Verify synced flag is STILL false (preserved)
        let clips = db.list_clips(None, None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(
            clips[0].synced, false,
            "synced should be preserved from local state"
        );
        assert_eq!(
            clips[0].content, "updated from relay",
            "mutable fields should be updated"
        );

        // 5. Verify unsynced list still shows it
        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 1);
    }
}
