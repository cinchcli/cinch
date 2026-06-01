//! Config schema migrations.
//!
//! The on-disk config (`~/.cinch/config.json`) carries a `config_version`
//! field (see [`crate::config::MultiConfig`]). This module owns the logic that
//! brings a config value read from disk up to the version this build
//! understands, *before* it is deserialized into a typed `MultiConfig`.
//!
//! Each schema bump is expressed as a discrete [`Migration`] that transforms a
//! raw `serde_json::Value` from one version to the next. Keeping migrations at
//! the `Value` level (rather than between typed structs) means an old field
//! that no longer exists on the current struct can still be read and remapped
//! during the upgrade — `serde` would otherwise drop it on deserialize.
//!
//! # Compatibility contract
//! - **Unversioned configs** (written before the `config_version` field
//!   existed, or carrying `config_version: 0`) are treated as **v1**. This
//!   keeps every existing installation loading unchanged.
//! - **Older versions** (`detected < current`) are migrated forward by applying
//!   each registered `(n) -> (n+1)` migration in sequence.
//! - **Newer versions** (`detected > current`) are *not* mutated: the value is
//!   returned as-is with a warning. A newer build wrote it; this older build
//!   loads it best-effort rather than corrupting fields it does not understand.
//!
//! # Safety
//! A migration MUST preserve the credential-bearing fields verbatim —
//! `token`, `encryption_key`, `device_private_key`. Losing any of them signs
//! the device out or makes its clips permanently undecryptable.

use serde_json::Value;

/// A single forward schema migration (version `N` -> `N + 1`). The version pair
/// a migration handles is fixed by its registration in [`migration_for`].
pub trait Migration {
    /// Transform a raw config value, preserving all credential fields.
    fn migrate(&self, value: Value) -> Result<Value, String>;
}

/// The config schema version encoded in `value`. Both a missing field and an
/// explicit `0` are interpreted as v1 (the first versioned schema), so configs
/// written before the version field existed continue to load.
pub fn detect_version(value: &Value) -> u32 {
    match value.get("config_version").and_then(Value::as_u64) {
        None | Some(0) => 1,
        Some(n) => n as u32,
    }
}

/// Bring `value` up to `current` by applying each registered migration in
/// order. Returns the value unchanged when it is already current or comes from
/// a newer build (logging a warning in the latter case).
pub fn apply_migrations(value: Value, current: u32) -> Result<Value, String> {
    let detected = detect_version(&value);

    if detected == current {
        return Ok(value);
    }
    if detected > current {
        log::warn!(
            "cinch config schema version {detected} is newer than this build supports ({current}); \
             loading best-effort. Upgrade cinch to avoid losing newer settings."
        );
        return Ok(value);
    }

    let mut value = value;
    for from in detected..current {
        let migration = migration_for(from, from + 1)?;
        value = migration.migrate(value)?;
    }
    Ok(value)
}

/// Look up the migration that advances `from` -> `to`.
///
/// The registry is intentionally empty while the schema sits at v1. When a v2
/// is introduced, turn this into a `match (from, to)` with an arm such as
/// `(1, 2) => Ok(Box::new(V1ToV2))`.
fn migration_for(from: u32, to: u32) -> Result<Box<dyn Migration>, String> {
    Err(format!(
        "no migration registered for config schema {from} -> {to}; please upgrade cinch"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn missing_version_field_is_v1() {
        let v = json!({ "active_relay_id": "r1", "relays": [] });
        assert_eq!(detect_version(&v), 1);
    }

    #[test]
    fn explicit_zero_is_treated_as_v1() {
        let v = json!({ "config_version": 0, "relays": [] });
        assert_eq!(detect_version(&v), 1);
    }

    #[test]
    fn explicit_version_is_read_verbatim() {
        let v = json!({ "config_version": 7, "relays": [] });
        assert_eq!(detect_version(&v), 7);
    }

    #[test]
    fn current_version_is_returned_unchanged() {
        let v = json!({ "config_version": 1, "active_relay_id": "r1", "relays": [] });
        assert_eq!(apply_migrations(v.clone(), 1).unwrap(), v);
    }

    #[test]
    fn unversioned_config_needs_no_migration_at_v1() {
        // Legacy file with no version field: detected as v1, current is v1, so
        // it is returned byte-for-byte (the version field is added later, at
        // save time, via the typed struct's serde default).
        let v = json!({ "active_relay_id": "r1", "relays": [] });
        let out = apply_migrations(v.clone(), 1).unwrap();
        assert_eq!(out, v);
        assert!(out.get("config_version").is_none());
    }

    #[test]
    fn newer_version_loads_best_effort_unchanged() {
        // A config written by a future build must not be mutated by this older
        // one — credential fields could be silently dropped otherwise.
        let v = json!({
            "config_version": 999,
            "active_relay_id": "r1",
            "relays": [{ "id": "r1", "token": "secret", "encryption_key": "key" }]
        });
        let out = apply_migrations(v.clone(), 1).expect("best-effort load");
        assert_eq!(out, v);
    }

    #[test]
    fn missing_migration_path_is_an_error() {
        // Asking to migrate across an unregistered gap surfaces an error rather
        // than silently dropping data.
        assert!(migration_for(1, 2).is_err());
    }

    #[test]
    fn downgrade_request_without_registered_migration_errors() {
        // detected (5) < current is impossible today, but if a future current
        // (say 6) lacked the 5->6 migration, apply_migrations must error, not
        // corrupt the file.
        let v = json!({ "config_version": 5, "relays": [] });
        assert!(apply_migrations(v, 6).is_err());
    }
}
