use client_core::store::{migration, Store};
use rusqlite::Connection;
use std::fs;

#[test]
fn imports_legacy_clips() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let old_dir = tmp.path().join("old");
    fs::create_dir_all(&old_dir).unwrap();
    let old_db = old_dir.join("cinch.db");

    // Seed the old DB with the legacy schema we expect from desktop.
    {
        let c = Connection::open(&old_db).unwrap();
        c.execute_batch(
            r#"
            CREATE TABLE clips (
              id TEXT PRIMARY KEY, source TEXT, source_key TEXT,
              content_type TEXT, content BLOB, media_path TEXT,
              byte_size INTEGER, created_at INTEGER, pinned INTEGER, pinned_at INTEGER
            );
            CREATE TABLE devices (
              id TEXT PRIMARY KEY, hostname TEXT, nickname TEXT, source_key TEXT,
              machine_id TEXT, public_key TEXT, paired_at INTEGER, last_push_at INTEGER,
              online INTEGER, refreshed_at INTEGER
            );
            CREATE TABLE retention_prefs (device_id TEXT PRIMARY KEY, days INTEGER);
            CREATE TABLE alert_prefs (source TEXT PRIMARY KEY, enabled INTEGER);
        "#,
        )
        .unwrap();
        c.execute(
            "INSERT INTO clips VALUES('01HXOLD001','atlas0',NULL,'text/plain', X'68690A', NULL, 3, 1700000000000, 0, NULL)",
            [],
        )
        .unwrap();
    }

    let new_db = tmp.path().join("new").join("store.db");
    let new_media = tmp.path().join("new").join("media");
    let store = Store::open(&new_db).unwrap();
    let imported = migration::import_legacy_if_present(&store, &new_media, Some(&old_db)).unwrap();
    assert_eq!(imported.as_deref(), Some(old_db.as_path()));

    // Idempotent — second call short-circuits (old_db no longer exists after .bak rename,
    // but the meta guard fires first).
    let again = migration::import_legacy_if_present(&store, &new_media, Some(&old_db)).unwrap();
    assert!(again.is_none());

    // Row reachable via queries.
    let rows =
        client_core::store::queries::list_clips(&store, None, None, None, None, None, false, 10)
            .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "01HXOLD001");

    // Old DB was renamed to .bak, not just .db.
    let bak = old_dir.join("cinch.db.bak");
    assert!(bak.exists(), "expected cinch.db.bak to exist at {bak:?}");
}
