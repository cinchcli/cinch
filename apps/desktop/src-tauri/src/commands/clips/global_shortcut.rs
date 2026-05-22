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

/// Testable inner: validate and persist a global shortcut string (T-03-06).
///
/// Validation rules:
/// 1. Must contain at least one modifier key (Cmd, Ctrl, Alt, Shift, etc.)
/// 2. Must contain at least one regular (non-modifier) key
fn set_global_shortcut_inner(db: &Database, shortcut: &str) -> Result<(), String> {
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
}
