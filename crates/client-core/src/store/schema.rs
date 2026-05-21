use rusqlite::Connection;

pub const CURRENT_SCHEMA_VERSION: i64 = 2;

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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn v1_to_v2_adds_synced_column() {
        let conn = Connection::open_in_memory().unwrap();
        // Seed meta table that apply_migrations needs.
        conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);")
            .unwrap();
        // Manually run only v1 to simulate a stopped-at-v1 database.
        migrate_v1(&conn).unwrap();

        // Insert a row before the migration — it must survive and gain synced=1.
        conn.execute(
            "INSERT INTO clips (id, source, content_type, created_at) VALUES ('old','s','text',0)",
            [],
        )
        .unwrap();

        // Sanity: synced column does not exist yet.
        let err = conn.execute(
            "INSERT INTO clips (id, source, content_type, created_at, synced) VALUES ('x','s','text',0,1)",
            [],
        );
        assert!(err.is_err(), "synced column should not exist in v1 schema");

        // Run the full migration chain.
        apply_migrations(&conn).unwrap();

        // Pre-existing row picked up the default.
        let old_synced: i64 = conn
            .query_row("SELECT synced FROM clips WHERE id='old'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            old_synced, 1,
            "pre-existing rows must get synced=1 after migration"
        );

        // New row with explicit synced=0 works.
        conn.execute(
            "INSERT INTO clips (id, source, content_type, created_at, synced) VALUES ('x','s','text',0,0)",
            [],
        )
        .expect("synced column should exist after v2 migration");
        let n: i64 = conn
            .query_row("SELECT synced FROM clips WHERE id='x'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn fresh_db_has_synced_column_with_default_true() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO clips (id, source, content_type, created_at) VALUES ('y','s','text',0)",
            [],
        )
        .unwrap();
        let synced: i64 = conn
            .query_row("SELECT synced FROM clips WHERE id='y'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(synced, 1, "new rows must default to synced=1");
    }
}
