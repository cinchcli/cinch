use rusqlite::Connection;

pub const CURRENT_SCHEMA_VERSION: i64 = 8;

pub fn apply_migrations(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
    )?;
    let current: i64 = conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if current < 1 {
        migrate_v1(conn)?;
    }
    if current < 2 {
        migrate_v2(conn)?;
    }
    if current < 3 {
        migrate_v3(conn)?;
    }
    if current < 4 {
        migrate_v4(conn)?;
    }
    if current < 5 {
        migrate_v5(conn)?;
    }
    if current < 6 {
        migrate_v6(conn)?;
    }
    if current < 7 {
        migrate_v7(conn)?;
    }
    if current < 8 {
        migrate_v8(conn)?;
    }
    Ok(())
}

fn migrate_v1(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE clips (
          id           TEXT PRIMARY KEY,
          source       TEXT NOT NULL,
          source_key   TEXT,
          content_type TEXT NOT NULL,
          content      BLOB,
          media_path   TEXT,
          byte_size    INTEGER NOT NULL DEFAULT 0,
          created_at   INTEGER NOT NULL,
          pinned       INTEGER NOT NULL DEFAULT 0,
          pinned_at    INTEGER
        );
        CREATE INDEX clips_created_idx ON clips(created_at DESC);
        CREATE INDEX clips_source_idx  ON clips(source, created_at DESC);
        CREATE INDEX clips_pinned_idx  ON clips(pinned) WHERE pinned = 1;

        CREATE VIRTUAL TABLE clips_fts USING fts5(
            content, content='clips', content_rowid='rowid'
        );

        CREATE TRIGGER clips_ai AFTER INSERT ON clips BEGIN
          INSERT INTO clips_fts(rowid, content) VALUES (new.rowid, COALESCE(new.content, ''));
        END;
        CREATE TRIGGER clips_ad AFTER DELETE ON clips BEGIN
          INSERT INTO clips_fts(clips_fts, rowid, content) VALUES('delete', old.rowid, COALESCE(old.content, ''));
        END;
        CREATE TRIGGER clips_au AFTER UPDATE ON clips BEGIN
          INSERT INTO clips_fts(clips_fts, rowid, content) VALUES('delete', old.rowid, COALESCE(old.content, ''));
          INSERT INTO clips_fts(rowid, content)            VALUES (new.rowid, COALESCE(new.content, ''));
        END;

        CREATE TABLE devices (
          id           TEXT PRIMARY KEY,
          hostname     TEXT NOT NULL,
          nickname     TEXT,
          source_key   TEXT,
          machine_id   TEXT,
          public_key   TEXT,
          paired_at    INTEGER,
          last_push_at INTEGER,
          online       INTEGER NOT NULL DEFAULT 0,
          refreshed_at INTEGER NOT NULL
        );

        CREATE TABLE retention_prefs (device_id TEXT PRIMARY KEY, days INTEGER NOT NULL);
        CREATE TABLE alert_prefs     (source    TEXT PRIMARY KEY, enabled INTEGER NOT NULL);

        INSERT INTO meta(key, value) VALUES('schema_version', '1');
    "#,
    )?;
    Ok(())
}

fn migrate_v3(conn: &Connection) -> rusqlite::Result<()> {
    // Replace the boolean `synced` with a three-state `sync_state` enum.
    // synced=1 → 'synced'; synced=0 → 'local'. Mapping 0→'local' (not
    // 'pending') is deliberate: clips queued under the old auto-send regime
    // were never explicitly chosen for sending, so the security-first cutover
    // must not transmit them after upgrade.
    conn.execute_batch(
        r#"
        BEGIN;
        ALTER TABLE clips ADD COLUMN sync_state TEXT NOT NULL DEFAULT 'synced';
        UPDATE clips SET sync_state = CASE WHEN synced = 1 THEN 'synced' ELSE 'local' END;
        DROP INDEX IF EXISTS clips_unsynced_idx;
        ALTER TABLE clips DROP COLUMN synced;
        CREATE INDEX clips_pending_idx ON clips(sync_state, created_at)
            WHERE sync_state = 'pending';
        UPDATE meta SET value = '3' WHERE key = 'schema_version';
        COMMIT;
    "#,
    )?;
    Ok(())
}

fn migrate_v2(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        ALTER TABLE clips ADD COLUMN synced INTEGER NOT NULL DEFAULT 1;
        CREATE INDEX clips_unsynced_idx ON clips(synced, created_at)
            WHERE synced = 0;
        UPDATE meta SET value = '2' WHERE key = 'schema_version';
    "#,
    )?;
    Ok(())
}

fn migrate_v4(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        ALTER TABLE clips ADD COLUMN source_app TEXT;
        ALTER TABLE clips ADD COLUMN source_url TEXT;
        UPDATE meta SET value = '4' WHERE key = 'schema_version';
    "#,
    )?;
    Ok(())
}

fn migrate_v5(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        ALTER TABLE clips ADD COLUMN source_app_id TEXT;
        UPDATE meta SET value = '5' WHERE key = 'schema_version';
    "#,
    )?;
    Ok(())
}

fn migrate_v6(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        ALTER TABLE clips ADD COLUMN label TEXT;
        UPDATE meta SET value = '6' WHERE key = 'schema_version';
    "#,
    )?;
    Ok(())
}

fn migrate_v7(conn: &Connection) -> rusqlite::Result<()> {
    // The pre-v7 `clips_fts` was an *external-content* FTS5 table
    // (`content='clips'`) that indexed EVERY clip's `content` column — including
    // image clips, whose content is base64 that tokenizes into noise (short
    // tokens like "hi") and pollutes text search. External-content tables also
    // require the fragile `'delete'` command, whose tokens drift out of sync on
    // SQLite rowid reuse, leaving stale matches that point at unrelated clips.
    //
    // v7 replaces it with a standard FTS5 table managed entirely by triggers:
    //   - only non-image clips are indexed (no base64 pollution), and
    //   - rows are removed with a plain `DELETE ... WHERE rowid = ?`, which can
    //     never drift the way the external-content `'delete'` command does.
    // The existing index is dropped and rebuilt from `clips`, which also clears
    // any drift already present in upgraded databases.
    conn.execute_batch(
        r#"
        BEGIN;
        DROP TRIGGER IF EXISTS clips_ai;
        DROP TRIGGER IF EXISTS clips_ad;
        DROP TRIGGER IF EXISTS clips_au;
        DROP TABLE IF EXISTS clips_fts;

        CREATE VIRTUAL TABLE clips_fts USING fts5(content);

        CREATE TRIGGER clips_ai AFTER INSERT ON clips
        WHEN new.content_type NOT LIKE 'image%'
        BEGIN
          INSERT INTO clips_fts(rowid, content)
            VALUES (new.rowid, CAST(COALESCE(new.content, '') AS TEXT));
        END;

        CREATE TRIGGER clips_ad AFTER DELETE ON clips
        BEGIN
          DELETE FROM clips_fts WHERE rowid = old.rowid;
        END;

        CREATE TRIGGER clips_au AFTER UPDATE ON clips
        BEGIN
          DELETE FROM clips_fts WHERE rowid = old.rowid;
          INSERT INTO clips_fts(rowid, content)
            SELECT new.rowid, CAST(COALESCE(new.content, '') AS TEXT)
            WHERE new.content_type NOT LIKE 'image%';
        END;

        INSERT INTO clips_fts(rowid, content)
          SELECT rowid, CAST(COALESCE(content, '') AS TEXT)
          FROM clips
          WHERE content_type NOT LIKE 'image%';

        UPDATE meta SET value = '7' WHERE key = 'schema_version';
        COMMIT;
    "#,
    )?;
    // Reclaim the disk freed by dropping the old image-polluted index (the
    // base64 tokens could be tens of MB). Best-effort and outside the
    // transaction above (VACUUM cannot run inside one): the schema change is
    // already committed, so a VACUUM failure (e.g. low disk) must not fail the
    // migration. Runs exactly once, on the v6→v7 upgrade.
    let _ = conn.execute_batch("VACUUM");
    Ok(())
}

fn migrate_v8(conn: &Connection) -> rusqlite::Result<()> {
    // Generic key/value app settings, shared by the desktop and CLI. Replaces
    // the desktop's separate `com.cinch.app/clips.db` settings table so both
    // front-ends read one store. See store::settings for key conventions.
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
          key   TEXT PRIMARY KEY,
          value TEXT NOT NULL
        );
        UPDATE meta SET value = '8' WHERE key = 'schema_version';
    "#,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn v1_to_current_migration_chain() {
        let conn = Connection::open_in_memory().unwrap();
        // Seed meta table that apply_migrations needs.
        conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        // Manually run only v1 to simulate a stopped-at-v1 database.
        migrate_v1(&conn).unwrap();

        // Insert a row before the migration — it must survive and get sync_state='synced'.
        conn.execute(
            "INSERT INTO clips (id, source, content_type, created_at) VALUES ('old','s','text',0)",
            [],
        )
        .unwrap();

        // Sanity: synced column does not exist yet in v1.
        let err = conn.execute(
            "INSERT INTO clips (id, source, content_type, created_at, synced) VALUES ('x','s','text',0,1)",
            [],
        );
        assert!(err.is_err(), "synced column should not exist in v1 schema");

        // Run the full migration chain (v1 → v2 → v3).
        apply_migrations(&conn).unwrap();

        // Pre-existing row (had synced=1 default) must come through as sync_state='synced'.
        let old_state: String = conn
            .query_row("SELECT sync_state FROM clips WHERE id='old'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            old_state, "synced",
            "pre-existing rows must get sync_state='synced' after full migration"
        );

        // synced column must be gone after v3.
        let err = conn.query_row("SELECT synced FROM clips WHERE id='old'", [], |r| {
            r.get::<_, i64>(0)
        });
        assert!(err.is_err(), "synced column must be dropped after v3");
    }

    #[test]
    fn fresh_db_has_sync_state_column_with_default_synced() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO clips (id, source, content_type, created_at) VALUES ('y','s','text',0)",
            [],
        )
        .unwrap();
        let state: String = conn
            .query_row("SELECT sync_state FROM clips WHERE id='y'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            state, "synced",
            "new rows must default to sync_state='synced'"
        );
        let source_app: Option<String> = conn
            .query_row("SELECT source_app FROM clips WHERE id='y'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let source_app_id: Option<String> = conn
            .query_row("SELECT source_app_id FROM clips WHERE id='y'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let source_url: Option<String> = conn
            .query_row("SELECT source_url FROM clips WHERE id='y'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(source_app_id, None);
        assert_eq!(source_app, None);
        assert_eq!(source_url, None);
    }

    #[test]
    fn fresh_db_fts_excludes_image_content_and_is_current_version() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn).unwrap();

        let version: i64 = conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        assert_eq!(CURRENT_SCHEMA_VERSION, 8);

        // A text clip and an image clip whose (base64-ish) content both tokenize
        // to include "penguin". Only the text clip may be indexed.
        conn.execute(
            "INSERT INTO clips(id,source,content_type,content,created_at) VALUES('t','s','text','penguin note',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO clips(id,source,content_type,content,created_at) VALUES('i','s','image','penguin base64',0)",
            [],
        )
        .unwrap();

        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clips_fts WHERE clips_fts MATCH 'penguin'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1, "image content must not pollute the FTS index");
    }

    #[test]
    fn fts_delete_removes_entry_no_drift() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO clips(id,source,content_type,content,created_at) VALUES('t','s','text','quokka',0)",
            [],
        )
        .unwrap();
        let before: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clips_fts WHERE clips_fts MATCH 'quokka'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(before, 1);

        conn.execute("DELETE FROM clips WHERE id='t'", []).unwrap();
        let after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clips_fts WHERE clips_fts MATCH 'quokka'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            after, 0,
            "deleting a clip must remove its FTS entry (no drift)"
        );
    }

    #[test]
    fn migrate_v7_rebuilds_fts_excluding_images() {
        // Build a legacy v6 database (external-content FTS that indexes every
        // clip's content, including images) and verify the upgrade to v7 cleans
        // the index in place.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        migrate_v1(&conn).unwrap();
        migrate_v2(&conn).unwrap();
        migrate_v3(&conn).unwrap();
        migrate_v4(&conn).unwrap();
        migrate_v5(&conn).unwrap();
        migrate_v6(&conn).unwrap();
        conn.execute("UPDATE meta SET value='6' WHERE key='schema_version'", [])
            .unwrap();

        conn.execute(
            "INSERT INTO clips(id,source,content_type,content,created_at) VALUES('txt','s','text','wombat note',0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO clips(id,source,content_type,content,created_at) VALUES('img','s','image','wombat base64',0)",
            [],
        )
        .unwrap();

        // The legacy schema indexes the image's content too — this is the bug.
        let legacy: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clips_fts WHERE clips_fts MATCH 'wombat'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            legacy, 2,
            "legacy v6 indexes image content (the pollution bug)"
        );

        // Upgrade. migrate_v7 must rebuild the index excluding images.
        apply_migrations(&conn).unwrap();

        let after: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clips_fts WHERE clips_fts MATCH 'wombat'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(after, 1, "after v7 only non-image content is indexed");

        let id: String = conn
            .query_row(
                "SELECT id FROM clips WHERE rowid IN (SELECT rowid FROM clips_fts WHERE clips_fts MATCH 'wombat')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(id, "txt", "the surviving match must be the text clip");
    }

    #[test]
    fn fresh_db_has_settings_table_and_is_v8() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn).unwrap();
        let version: i64 = conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(version, CURRENT_SCHEMA_VERSION);
        assert_eq!(CURRENT_SCHEMA_VERSION, 8);
        // The settings table exists and round-trips.
        conn.execute("INSERT INTO settings(key, value) VALUES ('k','v')", [])
            .unwrap();
        let v: String = conn
            .query_row("SELECT value FROM settings WHERE key='k'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "v");
    }

    #[test]
    fn v2_to_v3_maps_synced_to_sync_state() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        migrate_v1(&conn).unwrap();
        migrate_v2(&conn).unwrap();

        // synced=1 → 'synced'; synced=0 → 'local' (security-first: pre-cutover
        // offline-queued clips become local-only, never auto-sent after upgrade).
        conn.execute(
            "INSERT INTO clips (id, source, content_type, created_at, synced) VALUES ('s','x','text',0,1)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO clips (id, source, content_type, created_at, synced) VALUES ('u','x','text',0,0)",
            [],
        ).unwrap();

        apply_migrations(&conn).unwrap();

        let synced_state: String = conn
            .query_row("SELECT sync_state FROM clips WHERE id='s'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let unsynced_state: String = conn
            .query_row("SELECT sync_state FROM clips WHERE id='u'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(synced_state, "synced");
        assert_eq!(unsynced_state, "local");

        let err = conn.query_row("SELECT synced FROM clips WHERE id='s'", [], |r| {
            r.get::<_, i64>(0)
        });
        assert!(err.is_err(), "synced column must be dropped after v3");
    }
}
