use tauri::State;

use crate::SharedStore;
use client_core::store::settings;

// ---------------------------------------------------------------------------
// Global shortcut persistence (plan 03-04, D-08)
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
fn get_global_shortcut_inner(store: &client_core::store::Store) -> Result<String, String> {
    Ok(settings::global_shortcut(store)
        .map_err(|e| e.to_string())?
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
fn set_global_shortcut_inner(
    store: &client_core::store::Store,
    shortcut: &str,
) -> Result<(), String> {
    validate_shortcut(shortcut)?;
    settings::set_global_shortcut(store, shortcut).map_err(|e| e.to_string())
}

#[tauri::command]
#[specta::specta]
pub fn get_global_shortcut(store: State<'_, SharedStore>) -> Result<String, String> {
    get_global_shortcut_inner(&store)
}

#[tauri::command]
#[specta::specta]
pub fn set_global_shortcut(store: State<'_, SharedStore>, shortcut: String) -> Result<(), String> {
    set_global_shortcut_inner(&store, &shortcut)
}

// ---------------------------------------------------------------------------
// Send shortcut persistence (Task 3 — opt-in, no default)
// ---------------------------------------------------------------------------

/// No default: absence means the send hotkey is disabled (opt-in).
fn get_send_shortcut_inner(store: &client_core::store::Store) -> Result<Option<String>, String> {
    settings::send_shortcut(store).map_err(|e| e.to_string())
}

/// Testable inner: validate and persist the opt-in send shortcut.
/// `None` clears the key (disabling the hotkey); `Some(s)` validates then stores.
fn set_send_shortcut_inner(
    store: &client_core::store::Store,
    shortcut: Option<&str>,
) -> Result<(), String> {
    match shortcut {
        None => settings::delete_setting(store, "send_shortcut").map_err(|e| e.to_string()),
        Some(s) => {
            validate_shortcut(s)?;
            settings::set_send_shortcut(store, s).map_err(|e| e.to_string())
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn get_send_shortcut(store: State<'_, SharedStore>) -> Result<Option<String>, String> {
    get_send_shortcut_inner(&store)
}

#[tauri::command]
#[specta::specta]
pub fn set_send_shortcut(
    store: State<'_, SharedStore>,
    app: tauri::AppHandle,
    shortcut: Option<String>,
) -> Result<(), String> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    // Capture the previously-registered value before overwriting it.
    let prev = get_send_shortcut_inner(&store)?;
    set_send_shortcut_inner(&store, shortcut.as_deref())?;
    // Unregister ONLY the previous send shortcut (not unregister_all, which would
    // also drop the window-show shortcut), then re-register from the new value.
    if let Some(prev) = prev {
        let _ = app.global_shortcut().unregister(prev.as_str());
    }
    crate::window_manage::register_send_shortcut(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use client_core::store::Store;

    use super::*;

    fn test_store() -> Store {
        Store::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn global_shortcut_defaults_when_missing() {
        let store = test_store();
        let s = get_global_shortcut_inner(&store).unwrap();
        assert_eq!(s, DEFAULT_GLOBAL_SHORTCUT);
    }

    #[test]
    fn global_shortcut_roundtrip() {
        let store = test_store();
        set_global_shortcut_inner(&store, "CmdOrCtrl+Shift+B").unwrap();
        let s = get_global_shortcut_inner(&store).unwrap();
        assert_eq!(s, "CmdOrCtrl+Shift+B");
    }

    #[test]
    fn global_shortcut_rejects_no_modifier() {
        let store = test_store();
        let err = set_global_shortcut_inner(&store, "V").unwrap_err();
        assert!(
            err.contains("modifier"),
            "error should mention modifier: {}",
            err
        );
    }

    #[test]
    fn global_shortcut_rejects_modifier_only() {
        let store = test_store();
        let err = set_global_shortcut_inner(&store, "Cmd+Shift").unwrap_err();
        assert!(
            err.contains("regular key"),
            "error should mention regular key: {}",
            err
        );
    }

    #[test]
    fn global_shortcut_accepts_alt_combo() {
        let store = test_store();
        assert!(set_global_shortcut_inner(&store, "Alt+Space").is_ok());
        assert_eq!(get_global_shortcut_inner(&store).unwrap(), "Alt+Space");
    }

    #[test]
    fn send_shortcut_is_none_when_unset() {
        let store = test_store();
        assert_eq!(get_send_shortcut_inner(&store).unwrap(), None);
    }

    #[test]
    fn send_shortcut_roundtrip_and_clear() {
        let store = test_store();
        set_send_shortcut_inner(&store, Some("CmdOrCtrl+Shift+S")).unwrap();
        assert_eq!(
            get_send_shortcut_inner(&store).unwrap(),
            Some("CmdOrCtrl+Shift+S".to_string())
        );
        set_send_shortcut_inner(&store, None).unwrap(); // clearing returns to opt-out
        assert_eq!(get_send_shortcut_inner(&store).unwrap(), None);
    }

    #[test]
    fn send_shortcut_overwrite() {
        let store = test_store();
        set_send_shortcut_inner(&store, Some("CmdOrCtrl+Shift+S")).unwrap();
        set_send_shortcut_inner(&store, Some("Alt+F")).unwrap();
        assert_eq!(
            get_send_shortcut_inner(&store).unwrap(),
            Some("Alt+F".to_string())
        );
    }

    #[test]
    fn send_shortcut_rejects_no_modifier() {
        let store = test_store();
        let err = set_send_shortcut_inner(&store, Some("S")).unwrap_err();
        assert!(
            err.contains("modifier"),
            "error should mention modifier: {}",
            err
        );
    }
}
