//! Local key/value app settings, shared by the desktop and CLI via the single
//! store at `~/.cinch/store.db`. Owns the key conventions and default values
//! for every persisted setting so the desktop carries no setting strings.

use super::{Store, StoreError};
use rusqlite::{params, OptionalExtension};
#[cfg(feature = "specta")]
use specta::Type;

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
        // Escape LIKE metacharacters so a literal prefix is always matched.
        // Backslash must be replaced first to avoid double-escaping.
        let escaped = prefix
            .replace('\\', r"\\")
            .replace('%', r"\%")
            .replace('_', r"\_");
        let pattern = format!("{escaped}%");
        let mut stmt =
            conn.prepare("SELECT key, value FROM settings WHERE key LIKE ?1 ESCAPE '\\'")?;
        let rows = stmt
            .query_map(params![pattern], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })
}

/// Per-source auto-copy preference (frontend-facing).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(Type))]
pub struct SourceSetting {
    pub source: String,
    pub auto_copy: bool,
}

/// Per-source alert preference (frontend-facing).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(Type))]
pub struct SourceAlertSetting {
    pub source: String,
    pub alert_enabled: bool,
}

/// Key for persisted main-window placement (raw JSON owned by the desktop).
pub const WINDOW_PLACEMENT_KEY: &str = "window_placement";

// ── Source preferences ───────────────────────────────────────────────────

pub fn is_source_auto_copy(store: &Store, source: &str) -> Result<bool, StoreError> {
    Ok(get_setting(store, &format!("auto_copy:{source}"))?.as_deref() == Some("true"))
}

pub fn set_source_auto_copy(store: &Store, source: &str, enabled: bool) -> Result<(), StoreError> {
    set_setting(
        store,
        &format!("auto_copy:{source}"),
        if enabled { "true" } else { "false" },
    )
}

/// Defaults to `true` (alerts on) when unset.
pub fn is_source_alert_enabled(store: &Store, source: &str) -> Result<bool, StoreError> {
    match get_setting(store, &format!("alert_enabled:{source}"))? {
        Some(v) => Ok(v == "true"),
        None => Ok(true),
    }
}

pub fn set_source_alert_enabled(
    store: &Store,
    source: &str,
    enabled: bool,
) -> Result<(), StoreError> {
    set_setting(
        store,
        &format!("alert_enabled:{source}"),
        if enabled { "true" } else { "false" },
    )
}

pub fn all_source_settings(store: &Store) -> Result<Vec<SourceSetting>, StoreError> {
    Ok(list_settings_with_prefix(store, "auto_copy:")?
        .into_iter()
        .filter_map(|(k, v)| {
            k.strip_prefix("auto_copy:").map(|src| SourceSetting {
                source: src.to_string(),
                auto_copy: v == "true",
            })
        })
        .collect())
}

pub fn all_source_alert_settings(store: &Store) -> Result<Vec<SourceAlertSetting>, StoreError> {
    Ok(list_settings_with_prefix(store, "alert_enabled:")?
        .into_iter()
        .filter_map(|(k, v)| {
            k.strip_prefix("alert_enabled:")
                .map(|src| SourceAlertSetting {
                    source: src.to_string(),
                    alert_enabled: v == "true",
                })
        })
        .collect())
}

// ── Retention ──────────────────────────────────────────────────────────────

pub fn local_retention_days(store: &Store) -> Result<Option<i64>, StoreError> {
    Ok(get_setting(store, "local_retention_days")?.and_then(|v| v.parse().ok()))
}

pub fn set_local_retention_days(store: &Store, days: i64) -> Result<(), StoreError> {
    set_setting(store, "local_retention_days", &days.to_string())
}

pub fn remote_retention_days(store: &Store) -> Result<Option<i64>, StoreError> {
    Ok(get_setting(store, "remote_retention_days")?.and_then(|v| v.parse().ok()))
}

pub fn set_remote_retention_days(store: &Store, days: i64) -> Result<(), StoreError> {
    set_setting(store, "remote_retention_days", &days.to_string())
}

// ── Shortcuts ────────────────────────────────────────────────────────────────

pub fn global_shortcut(store: &Store) -> Result<Option<String>, StoreError> {
    get_setting(store, "global_shortcut")
}

pub fn set_global_shortcut(store: &Store, shortcut: &str) -> Result<(), StoreError> {
    set_setting(store, "global_shortcut", shortcut)
}

pub fn send_shortcut(store: &Store) -> Result<Option<String>, StoreError> {
    get_setting(store, "send_shortcut")
}

pub fn set_send_shortcut(store: &Store, shortcut: &str) -> Result<(), StoreError> {
    set_setting(store, "send_shortcut", shortcut)
}

// ── Excluded apps (clipboard monitoring) ────────────────────────────────────

pub fn excluded_apps(store: &Store) -> Result<Vec<String>, StoreError> {
    match get_setting(store, "excluded_apps")? {
        Some(json) => Ok(serde_json::from_str(&json).unwrap_or_default()),
        None => Ok(Vec::new()),
    }
}

pub fn set_excluded_apps(store: &Store, apps: &[String]) -> Result<(), StoreError> {
    // Vec<String> serialization is infallible; fall back to "[]" defensively.
    let json = serde_json::to_string(apps).unwrap_or_else(|_| "[]".to_string());
    set_setting(store, "excluded_apps", &json)
}

// ── Window placement (raw JSON; desktop owns the Placement struct) ──────────

pub fn window_placement(store: &Store) -> Result<Option<String>, StoreError> {
    get_setting(store, WINDOW_PLACEMENT_KEY)
}

pub fn set_window_placement(store: &Store, raw_json: &str) -> Result<(), StoreError> {
    set_setting(store, WINDOW_PLACEMENT_KEY, raw_json)
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

    #[test]
    fn source_auto_copy_defaults_false_and_toggles() {
        let s = store();
        assert!(!is_source_auto_copy(&s, "remote:prod").unwrap());
        set_source_auto_copy(&s, "remote:prod", true).unwrap();
        assert!(is_source_auto_copy(&s, "remote:prod").unwrap());
    }

    #[test]
    fn source_alert_defaults_true_and_toggles() {
        let s = store();
        assert!(is_source_alert_enabled(&s, "remote:prod").unwrap());
        set_source_alert_enabled(&s, "remote:prod", false).unwrap();
        assert!(!is_source_alert_enabled(&s, "remote:prod").unwrap());
    }

    #[test]
    fn all_source_settings_lists_rows() {
        let s = store();
        set_source_auto_copy(&s, "remote:a", true).unwrap();
        let rows = all_source_settings(&s).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, "remote:a");
        assert!(rows[0].auto_copy);
    }

    #[test]
    fn retention_days_round_trip() {
        let s = store();
        assert_eq!(local_retention_days(&s).unwrap(), None);
        set_local_retention_days(&s, 30).unwrap();
        assert_eq!(local_retention_days(&s).unwrap(), Some(30));
        set_remote_retention_days(&s, 7).unwrap();
        assert_eq!(remote_retention_days(&s).unwrap(), Some(7));
    }

    #[test]
    fn shortcuts_round_trip() {
        let s = store();
        assert_eq!(global_shortcut(&s).unwrap(), None);
        set_global_shortcut(&s, "Cmd+Shift+V").unwrap();
        assert_eq!(
            global_shortcut(&s).unwrap(),
            Some("Cmd+Shift+V".to_string())
        );
    }

    #[test]
    fn excluded_apps_json_round_trip() {
        let s = store();
        assert_eq!(excluded_apps(&s).unwrap(), Vec::<String>::new());
        set_excluded_apps(
            &s,
            &["com.1password".to_string(), "com.keychain".to_string()],
        )
        .unwrap();
        assert_eq!(
            excluded_apps(&s).unwrap(),
            vec!["com.1password".to_string(), "com.keychain".to_string()]
        );
    }

    #[test]
    fn window_placement_raw_passthrough() {
        let s = store();
        assert_eq!(window_placement(&s).unwrap(), None);
        set_window_placement(&s, "{\"x\":1}").unwrap();
        assert_eq!(window_placement(&s).unwrap(), Some("{\"x\":1}".to_string()));
    }

    #[test]
    fn list_settings_with_prefix_treats_metacharacters_literally() {
        let s = store();
        set_setting(&s, "foo%bar", "a").unwrap();
        set_setting(&s, "fooXbar", "b").unwrap();
        let rows = list_settings_with_prefix(&s, "foo%").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "foo%bar");
    }

    #[test]
    fn all_source_alert_settings_lists_rows() {
        let s = store();
        set_source_alert_enabled(&s, "remote:b", false).unwrap();
        let rows = all_source_alert_settings(&s).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, "remote:b");
        assert!(!rows[0].alert_enabled);
    }

    #[test]
    fn send_shortcut_round_trip() {
        let s = store();
        assert_eq!(send_shortcut(&s).unwrap(), None);
        set_send_shortcut(&s, "Cmd+Shift+S").unwrap();
        assert_eq!(send_shortcut(&s).unwrap(), Some("Cmd+Shift+S".to_string()));
    }
}
