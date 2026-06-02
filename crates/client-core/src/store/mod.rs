//! Local SQLite store for clips, devices, prefs. Shared by CLI and desktop.

pub mod clips;
pub mod devices;
pub mod migration;
pub mod models;
pub mod prefix;
pub mod queries;
pub mod retention;
pub mod schema;
pub mod search;
pub mod settings;
pub mod sync_state;

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub struct Store {
    conn: Mutex<Connection>,
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("migration: {0}")]
    Migration(String),
}

impl Store {
    /// Open or create a store at `path`. Applies pending migrations.
    /// `:memory:` is recognised for tests.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let is_memory = path == Path::new(":memory:");
        let conn = if is_memory {
            Connection::open_in_memory()?
        } else {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            Connection::open(path)?
        };
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        schema::apply_migrations(&conn)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        // One-shot import from the desktop's legacy SQLite if present.
        // Idempotent; skipped for in-memory test stores.
        if !is_memory {
            if let Ok(media) = default_media_root() {
                let _ = migration::import_legacy_if_present(&store, &media, None);
            }
        }
        Ok(store)
    }

    pub(crate) fn with_conn<R>(
        &self,
        f: impl FnOnce(&Connection) -> Result<R, rusqlite::Error>,
    ) -> Result<R, StoreError> {
        let guard = self.conn.lock().expect("store mutex poisoned");
        Ok(f(&guard)?)
    }
}

/// Returns `<home>/.cinch`, creating it if necessary. Used as the storage
/// root for both CLI and desktop on every supported platform.
pub fn cinch_dir() -> std::io::Result<PathBuf> {
    let home = dirs::home_dir()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "home_dir unavailable"))?;
    let dir = home.join(".cinch");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn default_db_path() -> std::io::Result<PathBuf> {
    Ok(cinch_dir()?.join("store.db"))
}

pub fn default_media_root() -> std::io::Result<PathBuf> {
    let dir = cinch_dir()?.join("media");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn lock_path() -> std::io::Result<PathBuf> {
    Ok(cinch_dir()?.join("sync.lock"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_in_memory_and_runs_migrations() {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let version: i64 = store
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(version, schema::CURRENT_SCHEMA_VERSION);
    }

    #[test]
    fn insert_and_list_clips_round_trip() {
        use super::models::StoredClip;
        let store = Store::open(Path::new(":memory:")).unwrap();
        let clip = StoredClip {
            id: "01HXABC".into(),
            source: "atlas0".into(),
            source_key: None,
            source_app_id: None,
            source_app: None,
            source_url: None,
            label: None,
            content_type: "text/plain".into(),
            content: Some(b"hello".to_vec()),
            media_path: None,
            byte_size: 5,
            created_at: 1_700_000_000_000,
            pinned: false,
            pinned_at: None,
            sync_state: crate::store::models::SyncState::Synced,
        };
        super::queries::insert_clip(&store, &clip).unwrap();
        let rows = super::queries::list_clips(&store, None, None, None, None, false, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "01HXABC");
        assert_eq!(rows[0].content.as_deref(), Some(b"hello" as &[u8]));
    }

    #[test]
    fn search_finds_text_clips() {
        use super::models::StoredClip;
        let store = Store::open(Path::new(":memory:")).unwrap();
        for (i, body) in ["hello world", "foo bar", "hello foo"].iter().enumerate() {
            super::queries::insert_clip(
                &store,
                &StoredClip {
                    id: format!("01HXABC{i:03}"),
                    source: "m".into(),
                    source_key: None,
                    source_app_id: None,
                    source_app: None,
                    source_url: None,
                    label: None,
                    content_type: "text/plain".into(),
                    content: Some(body.as_bytes().to_vec()),
                    media_path: None,
                    byte_size: body.len() as i64,
                    created_at: 1_700_000_000_000 + i as i64,
                    pinned: false,
                    pinned_at: None,
                    sync_state: crate::store::models::SyncState::Synced,
                },
            )
            .unwrap();
        }
        let hits = super::queries::search_clips(&store, "hello", 10, None).unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn resolve_prefix_handles_ambiguity() {
        use super::models::{ResolveError, StoredClip};
        let store = Store::open(Path::new(":memory:")).unwrap();
        for id in ["01HXABC001", "01HXABC002", "01HXDEF003"] {
            super::queries::insert_clip(
                &store,
                &StoredClip {
                    id: id.into(),
                    source: "m".into(),
                    source_key: None,
                    source_app_id: None,
                    source_app: None,
                    source_url: None,
                    label: None,
                    content_type: "text/plain".into(),
                    content: Some(b"x".to_vec()),
                    media_path: None,
                    byte_size: 1,
                    created_at: 0,
                    pinned: false,
                    pinned_at: None,
                    sync_state: crate::store::models::SyncState::Synced,
                },
            )
            .unwrap();
        }
        assert!(matches!(
            super::prefix::resolve_clip_id(&store, "01H"),
            Err(ResolveError::TooShort)
        ));
        assert!(matches!(
            super::prefix::resolve_clip_id(&store, "9999"),
            Err(ResolveError::NotFound)
        ));
        let dup = super::prefix::resolve_clip_id(&store, "01HXABC");
        assert!(matches!(dup, Err(ResolveError::Ambiguous { .. })));
        let exact = super::prefix::resolve_clip_id(&store, "01HXDEF003").unwrap();
        assert_eq!(exact, "01HXDEF003");
    }
}
