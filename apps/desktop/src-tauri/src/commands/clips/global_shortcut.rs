use std::sync::Arc;

use tauri::State;

use crate::store::db::Database;

// ---------------------------------------------------------------------------
// Global shortcut persistence (plan 03-04, D-08)
// TODO(phase 5): move to client-core meta/settings table.
// ---------------------------------------------------------------------------

const DEFAULT_GLOBAL_SHORTCUT: &str = "CmdOrCtrl+Shift+V";

/// Modifier key names recognized by Tauri's global-shortcut plugin.
const MODIFIER_NAMES: &[&str] = &[
    "cmd",
    "ctrl",
    "alt",
    "shift",
    "super",
    "meta",
    "commandorcontrol",
    "cmdorctrl",
];

/// Testable inner: read persisted global shortcut or return the default.
fn get_global_shortcut_inner(db: &Database) -> Result<String, String> {
    Ok(db
        .get_setting("global_shortcut")?
        .unwrap_or_else(|| DEFAULT_GLOBAL_SHORTCUT.to_string()))
}

/// Validate a global-shortcut string: must contain at least one modifier AND
/// at least one regular (non-modifier) key.
fn validate_shortcut(shortcut: &str) -> Result<(), String> {
    let parts: Vec<&str> = shortcut.split('+').collect();
    let has_modifier = parts
        .iter()
        .any(|p| MODIFIER_NAMES.contains(&p.to_lowercase().as_str()));
    if !has_modifier {
        return Err(
            "Shortcut must include at least one modifier key (Cmd, Ctrl, Alt, Shift)".to_string(),
        );
    }
    let has_regular_key = parts
        .iter()
        .any(|p| !MODIFIER_NAMES.contains(&p.to_lowercase().as_str()));
    if !has_regular_key {
        return Err("Shortcut must include a regular key (e.g., V, C, Space)".to_string());
    }
    Ok(())
}

/// Testable inner: validate and persist a global shortcut string (T-03-06).
///
/// Validation rules:
/// 1. Must contain at least one modifier key (Cmd, Ctrl, Alt, Shift, etc.)
/// 2. Must contain at least one regular (non-modifier) key
fn set_global_shortcut_inner(db: &Database, shortcut: &str) -> Result<(), String> {
    validate_shortcut(shortcut)?;
    db.set_setting("global_shortcut", shortcut)
}

#[tauri::command]
#[specta::specta]
pub fn get_global_shortcut(db: State<'_, Arc<Database>>) -> Result<String, String> {
    get_global_shortcut_inner(&db)
}

#[tauri::command]
#[specta::specta]
pub fn set_global_shortcut(db: State<'_, Arc<Database>>, shortcut: String) -> Result<(), String> {
    set_global_shortcut_inner(&db, &shortcut)
}

// ---------------------------------------------------------------------------
// Send shortcut persistence (Task 3 — opt-in, no default)
// ---------------------------------------------------------------------------

const SEND_SHORTCUT_KEY: &str = "send_shortcut";

/// No default: absence means the send hotkey is disabled (opt-in).
fn get_send_shortcut_inner(db: &Database) -> Result<Option<String>, String> {
    db.get_setting(SEND_SHORTCUT_KEY)
}

fn set_send_shortcut_inner(db: &Database, shortcut: Option<&str>) -> Result<(), String> {
    match shortcut {
        None => db.delete_setting(SEND_SHORTCUT_KEY),
        Some(s) => {
            validate_shortcut(s)?;
            db.set_setting(SEND_SHORTCUT_KEY, s)
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_send_shortcut(db: State<'_, Arc<Database>>) -> Result<Option<String>, String> {
    get_send_shortcut_inner(&db)
}

#[tauri::command]
#[specta::specta]
pub fn set_send_shortcut(
    db: State<'_, Arc<Database>>,
    shortcut: Option<String>,
) -> Result<(), String> {
    set_send_shortcut_inner(&db, shortcut.as_deref())
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::test_db;
    use super::*;

    #[test]
    fn global_shortcut_defaults_when_missing() {
        let db = test_db();
        let s = get_global_shortcut_inner(&db).unwrap();
        assert_eq!(s, DEFAULT_GLOBAL_SHORTCUT);
    }

    #[test]
    fn global_shortcut_roundtrip() {
        let db = test_db();
        set_global_shortcut_inner(&db, "CmdOrCtrl+Shift+B").unwrap();
        let s = get_global_shortcut_inner(&db).unwrap();
        assert_eq!(s, "CmdOrCtrl+Shift+B");
    }

    #[test]
    fn global_shortcut_rejects_no_modifier() {
        let db = test_db();
        let err = set_global_shortcut_inner(&db, "V").unwrap_err();
        assert!(
            err.contains("modifier"),
            "error should mention modifier: {}",
            err
        );
    }

    #[test]
    fn global_shortcut_rejects_modifier_only() {
        let db = test_db();
        let err = set_global_shortcut_inner(&db, "Cmd+Shift").unwrap_err();
        assert!(
            err.contains("regular key"),
            "error should mention regular key: {}",
            err
        );
    }

    #[test]
    fn global_shortcut_accepts_alt_combo() {
        let db = test_db();
        assert!(set_global_shortcut_inner(&db, "Alt+Space").is_ok());
        assert_eq!(get_global_shortcut_inner(&db).unwrap(), "Alt+Space");
    }

    #[test]
    fn send_shortcut_is_none_when_unset() {
        let db = test_db();
        assert_eq!(get_send_shortcut_inner(&db).unwrap(), None);
    }

    #[test]
    fn send_shortcut_roundtrip_and_clear() {
        let db = test_db();
        set_send_shortcut_inner(&db, Some("CmdOrCtrl+Shift+S")).unwrap();
        assert_eq!(
            get_send_shortcut_inner(&db).unwrap(),
            Some("CmdOrCtrl+Shift+S".to_string())
        );
        set_send_shortcut_inner(&db, None).unwrap(); // clearing returns to opt-out
        assert_eq!(get_send_shortcut_inner(&db).unwrap(), None);
    }

    #[test]
    fn send_shortcut_rejects_no_modifier() {
        let db = test_db();
        assert!(set_send_shortcut_inner(&db, Some("S")).is_err());
    }
}
