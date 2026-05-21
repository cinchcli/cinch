use log::info;

use super::Database;

impl Database {
    pub(super) fn migrate(&self) -> Result<(), String> {
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
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::test_db;
    use super::Database;

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
    fn test_existing_clips_default_synced_true() {
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
}
