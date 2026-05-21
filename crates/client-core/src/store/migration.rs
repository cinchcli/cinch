use super::{Store, StoreError};
use std::path::{Path, PathBuf};

/// Returns the platform-specific path where the desktop app stored its
/// SQLite database before consolidation.
pub fn legacy_desktop_db_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    #[cfg(target_os = "macos")]
    return Some(home.join("Library/Application Support/com.cinchcli.desktop/cinch.db"));
    #[cfg(target_os = "linux")]
    return Some(home.join(".local/share/com.cinchcli.desktop/cinch.db"));
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            return Some(
                PathBuf::from(appdata)
                    .join("com.cinchcli.desktop")
                    .join("cinch.db"),
            );
        }
        return None;
    }
    #[allow(unreachable_code)]
    None
}

/// Import the old desktop DB into the new store if present. Idempotent —
/// safe to call multiple times; it short-circuits once `meta.migrated_from`
/// is set.
///
/// Returns the path that was imported, if any.
pub fn import_legacy_if_present(
    store: &Store,
    new_media_root: &Path,
    legacy_path: Option<&Path>,
) -> Result<Option<PathBuf>, StoreError> {
    let already: Option<String> = store.with_conn(|c| {
        c.query_row(
            "SELECT value FROM meta WHERE key='migrated_from'",
            [],
            |r| r.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
    })?;
    if already.is_some() {
        return Ok(None);
    }

    let path = match legacy_path {
        Some(p) => p.to_path_buf(),
        None => match legacy_desktop_db_path() {
            Some(p) => p,
            None => return Ok(None),
        },
    };
    if !path.exists() {
        return Ok(None);
    }

    // Run import in one transaction.
    store.with_conn(|conn| {
        conn.execute_batch(&format!(
            "ATTACH DATABASE {p} AS old;
             BEGIN;
             INSERT OR REPLACE INTO clips
               (id, source, source_key, content_type, content, media_path, byte_size, created_at, pinned, pinned_at)
               SELECT id, source, source_key, content_type, content, media_path, byte_size, created_at,
                      COALESCE(pinned, 0), pinned_at
                 FROM old.clips;
             INSERT OR REPLACE INTO devices
               SELECT id, hostname, nickname, source_key, machine_id, public_key,
                      paired_at, last_push_at, online, refreshed_at FROM old.devices;
             INSERT OR REPLACE INTO retention_prefs SELECT device_id, days FROM old.retention_prefs;
             INSERT OR REPLACE INTO alert_prefs     SELECT source, enabled  FROM old.alert_prefs;
             INSERT OR REPLACE INTO meta(key, value)
               VALUES('migrated_from', {p});
             COMMIT;
             DETACH DATABASE old;",
            p = sql_literal(&path.to_string_lossy())
        ))?;
        Ok(())
    })?;

    // Move media files alongside the new DB.
    if let Some(parent) = path.parent() {
        let old_media = parent.join("media");
        if old_media.exists() {
            std::fs::create_dir_all(new_media_root)?;
            for entry in std::fs::read_dir(&old_media)? {
                let e = entry?;
                let dest = new_media_root.join(e.file_name());
                if std::fs::rename(e.path(), &dest).is_err() {
                    std::fs::copy(e.path(), &dest)?;
                }
            }
        }
    }

    // Rename old DB to .bak for recovery.
    // Use explicit string concatenation to get `cinch.db.bak` (not `cinch.bak`).
    let bak = path.parent().unwrap_or_else(|| Path::new("")).join(format!(
        "{}.bak",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = std::fs::rename(&path, bak);
    Ok(Some(path))
}

fn sql_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}
