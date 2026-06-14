use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use super::global_shortcut::MODIFIER_NAMES;
use crate::SharedStore;
use client_core::store::settings;

// ---------------------------------------------------------------------------
// In-app clip-action shortcuts (Edit / Copy / Pin / Send)
//
// Unlike the OS-global shortcuts in `global_shortcut.rs`, these only fire
// inside the app (a clip is selected, no text field focused). They are matched
// in the desktop keydown handler and never registered with the OS — so this
// module only persists/validates; there is no register/unregister step.
// ---------------------------------------------------------------------------

// Edit moves off the bare "E" key to "CmdOrCtrl+E" so a single keystroke no
// longer opens the editor by accident. Copy keeps the natural bare Enter; Pin
// and Send keep their established modifier combos.
const DEFAULT_EDIT: &str = "CmdOrCtrl+E";
const DEFAULT_COPY: &str = "Enter";
const DEFAULT_PIN: &str = "CmdOrCtrl+P";
const DEFAULT_SEND: &str = "CmdOrCtrl+Enter";

/// The four user-customizable in-app clip-action shortcuts. Persisted as one
/// JSON blob under the `action_shortcuts` settings key.
///
/// Fields are required on the wire (specta emits non-optional TS strings); a
/// partial/legacy stored blob is tolerated by merging through
/// `PartialActionShortcuts` in the getter rather than via `#[serde(default)]`
/// (which would make every generated TS field optional).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct ActionShortcuts {
    pub edit: String,
    pub copy: String,
    pub pin: String,
    pub send: String,
}

impl Default for ActionShortcuts {
    fn default() -> Self {
        Self {
            edit: DEFAULT_EDIT.to_string(),
            copy: DEFAULT_COPY.to_string(),
            pin: DEFAULT_PIN.to_string(),
            send: DEFAULT_SEND.to_string(),
        }
    }
}

/// Internal-only deserialization target for a possibly-partial stored blob.
/// Not a specta `Type` — it never crosses the boundary, so it can carry
/// `Option` fields without affecting the generated TS.
#[derive(Deserialize, Default)]
struct PartialActionShortcuts {
    edit: Option<String>,
    copy: Option<String>,
    pin: Option<String>,
    send: Option<String>,
}

impl PartialActionShortcuts {
    /// Fill any missing field from `ActionShortcuts::default()`.
    fn into_complete(self) -> ActionShortcuts {
        let d = ActionShortcuts::default();
        ActionShortcuts {
            edit: self.edit.unwrap_or(d.edit),
            copy: self.copy.unwrap_or(d.copy),
            pin: self.pin.unwrap_or(d.pin),
            send: self.send.unwrap_or(d.send),
        }
    }
}

/// Validate one in-app action shortcut: a real (non-modifier) key is REQUIRED,
/// but a modifier is OPTIONAL. This deliberately differs from
/// `global_shortcut::validate_shortcut` (which mandates a modifier): these
/// shortcuts only fire when a clip is selected and no field is focused, so a
/// bare key like "Enter" is safe. Empty and modifier-only strings are rejected.
fn validate_action_shortcut(shortcut: &str) -> Result<(), String> {
    let parts: Vec<&str> = shortcut.split('+').filter(|p| !p.is_empty()).collect();
    if parts.is_empty() {
        return Err("Shortcut must not be empty".to_string());
    }
    let has_regular_key = parts
        .iter()
        .any(|p| !MODIFIER_NAMES.contains(&p.to_lowercase().as_str()));
    if !has_regular_key {
        return Err("Shortcut must include a regular key (e.g., E, Enter, Space)".to_string());
    }
    Ok(())
}

/// Testable inner: read the persisted set, merged over defaults. A corrupt blob
/// falls back to defaults rather than erroring.
fn get_action_shortcuts_inner(
    store: &client_core::store::Store,
) -> Result<ActionShortcuts, String> {
    match settings::action_shortcuts(store).map_err(|e| e.to_string())? {
        Some(json) => Ok(serde_json::from_str::<PartialActionShortcuts>(&json)
            .unwrap_or_default()
            .into_complete()),
        None => Ok(ActionShortcuts::default()),
    }
}

/// Testable inner: validate all four, then persist as JSON.
fn set_action_shortcuts_inner(
    store: &client_core::store::Store,
    shortcuts: &ActionShortcuts,
) -> Result<(), String> {
    for s in [
        &shortcuts.edit,
        &shortcuts.copy,
        &shortcuts.pin,
        &shortcuts.send,
    ] {
        validate_action_shortcut(s)?;
    }
    let json = serde_json::to_string(shortcuts).map_err(|e| e.to_string())?;
    settings::set_action_shortcuts(store, &json).map_err(|e| e.to_string())
}

/// Testable inner: clear the stored override and return the defaults.
fn reset_action_shortcuts_inner(
    store: &client_core::store::Store,
) -> Result<ActionShortcuts, String> {
    settings::delete_setting(store, "action_shortcuts").map_err(|e| e.to_string())?;
    Ok(ActionShortcuts::default())
}

#[tauri::command]
#[specta::specta]
pub fn get_action_shortcuts(store: State<'_, SharedStore>) -> Result<ActionShortcuts, String> {
    get_action_shortcuts_inner(&store)
}

#[tauri::command]
#[specta::specta]
pub fn set_action_shortcuts(
    store: State<'_, SharedStore>,
    shortcuts: ActionShortcuts,
) -> Result<(), String> {
    set_action_shortcuts_inner(&store, &shortcuts)
}

#[tauri::command]
#[specta::specta]
pub fn reset_action_shortcuts(store: State<'_, SharedStore>) -> Result<ActionShortcuts, String> {
    reset_action_shortcuts_inner(&store)
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_core::store::Store;

    fn test_store() -> Store {
        Store::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn defaults_when_missing() {
        let store = test_store();
        assert_eq!(
            get_action_shortcuts_inner(&store).unwrap(),
            ActionShortcuts::default()
        );
    }

    #[test]
    fn roundtrip() {
        let store = test_store();
        let custom = ActionShortcuts {
            edit: "CmdOrCtrl+K".into(),
            copy: "Enter".into(),
            pin: "CmdOrCtrl+P".into(),
            send: "CmdOrCtrl+Enter".into(),
        };
        set_action_shortcuts_inner(&store, &custom).unwrap();
        assert_eq!(get_action_shortcuts_inner(&store).unwrap(), custom);
    }

    #[test]
    fn partial_json_merges_defaults() {
        let store = test_store();
        // A stored blob missing the `send` field must still complete to a
        // valid set via PartialActionShortcuts::into_complete().
        settings::set_action_shortcuts(
            &store,
            r#"{"edit":"CmdOrCtrl+K","copy":"Enter","pin":"CmdOrCtrl+P"}"#,
        )
        .unwrap();
        let got = get_action_shortcuts_inner(&store).unwrap();
        assert_eq!(got.edit, "CmdOrCtrl+K");
        assert_eq!(got.send, DEFAULT_SEND);
    }

    #[test]
    fn validate_accepts_bare_keys() {
        assert!(validate_action_shortcut("Enter").is_ok());
        assert!(validate_action_shortcut("E").is_ok());
        assert!(validate_action_shortcut("CmdOrCtrl+E").is_ok());
    }

    #[test]
    fn validate_rejects_empty() {
        assert!(validate_action_shortcut("").is_err());
    }

    #[test]
    fn validate_rejects_modifier_only() {
        let err = validate_action_shortcut("CmdOrCtrl+Shift").unwrap_err();
        assert!(err.contains("regular key"), "got: {err}");
    }

    #[test]
    fn set_rejects_invalid_member() {
        let store = test_store();
        let bad = ActionShortcuts {
            edit: "CmdOrCtrl+Shift".into(),
            ..Default::default()
        };
        assert!(set_action_shortcuts_inner(&store, &bad).is_err());
        // Nothing should have been persisted.
        assert_eq!(
            get_action_shortcuts_inner(&store).unwrap(),
            ActionShortcuts::default()
        );
    }

    #[test]
    fn reset_clears_to_defaults() {
        let store = test_store();
        let custom = ActionShortcuts {
            edit: "CmdOrCtrl+K".into(),
            ..Default::default()
        };
        set_action_shortcuts_inner(&store, &custom).unwrap();
        let got = reset_action_shortcuts_inner(&store).unwrap();
        assert_eq!(got, ActionShortcuts::default());
        assert_eq!(
            get_action_shortcuts_inner(&store).unwrap(),
            ActionShortcuts::default()
        );
    }
}
