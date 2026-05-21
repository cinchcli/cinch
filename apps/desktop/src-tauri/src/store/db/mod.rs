use log::info;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use specta::Type;

mod clips;
mod migrations;
mod pinning;
mod retention;
mod settings;
mod sync_queue;

pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create db dir: {}", e))?;
        }

        let conn = Connection::open(path).map_err(|e| format!("failed to open db: {}", e))?;

        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| format!("failed to set pragmas: {}", e))?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.migrate()?;

        info!("database opened: {}", path.display());
        Ok(db)
    }
}

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
pub(super) mod test_helpers {
    use super::Database;
    use crate::store::models::LocalClip;

    pub fn test_db() -> Database {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let tmp = std::env::temp_dir().join(format!("cinch-test-{}-{}.db", std::process::id(), n));
        let _ = std::fs::remove_file(&tmp);
        Database::open(&tmp).unwrap()
    }

    pub fn make_clip(id: &str, content: &str, source: &str, content_type: &str) -> LocalClip {
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
}
