//! Local key/value app settings, shared by the desktop and CLI via the single
//! store at `~/.cinch/store.db`. Owns the key conventions and default values
//! for every persisted setting so the desktop carries no setting strings.

use super::{Store, StoreError};
use rusqlite::{params, OptionalExtension};

/// Read a raw setting value, or `None` if unset.
pub fn get_setting(store: &Store, key: &str) -> Result<Option<String>, StoreError> {
    store.with_conn(|conn| {
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |r| r.get::<_, String>(0),
        )
        .optional()
    })
}

/// Upsert a raw setting value.
pub fn set_setting(store: &Store, key: &str, value: &str) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute(
            "INSERT OR REPLACE INTO settings(key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    })
}

/// Remove a setting (no-op if absent).
pub fn delete_setting(store: &Store, key: &str) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(())
    })
}

/// All `(key, value)` pairs whose key starts with `prefix`. Mirrors the
/// desktop's historical `WHERE key LIKE '<prefix>%'` scans.
pub fn list_settings_with_prefix(
    store: &Store,
    prefix: &str,
) -> Result<Vec<(String, String)>, StoreError> {
    store.with_conn(|conn| {
        let pattern = format!("{prefix}%");
        let mut stmt = conn.prepare("SELECT key, value FROM settings WHERE key LIKE ?1")?;
        let rows = stmt
            .query_map(params![pattern], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn get_set_delete_round_trip() {
        let s = store();
        assert_eq!(get_setting(&s, "k").unwrap(), None);
        set_setting(&s, "k", "v").unwrap();
        assert_eq!(get_setting(&s, "k").unwrap(), Some("v".to_string()));
        set_setting(&s, "k", "v2").unwrap();
        assert_eq!(get_setting(&s, "k").unwrap(), Some("v2".to_string()));
        delete_setting(&s, "k").unwrap();
        assert_eq!(get_setting(&s, "k").unwrap(), None);
    }

    #[test]
    fn list_with_prefix_returns_matching_pairs() {
        let s = store();
        set_setting(&s, "auto_copy:remote:a", "true").unwrap();
        set_setting(&s, "auto_copy:remote:b", "false").unwrap();
        set_setting(&s, "global_shortcut", "Cmd+X").unwrap();
        let mut got = list_settings_with_prefix(&s, "auto_copy:").unwrap();
        got.sort();
        assert_eq!(
            got,
            vec![
                ("auto_copy:remote:a".to_string(), "true".to_string()),
                ("auto_copy:remote:b".to_string(), "false".to_string()),
            ]
        );
    }
}
