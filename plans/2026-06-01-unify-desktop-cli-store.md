# Unify desktop + CLI onto one store — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the desktop app and the `cinch` CLI use the single shared store at `~/.cinch/store.db`, moving all app-settings handling into `client-core` and deleting the desktop's legacy `store::db` module.

**Architecture:** Approach C with a clean cutover (no legacy-data import). `client-core` gains a `settings(key,value)` table (schema v8) and a `store::settings` module that owns every setting's key, default, and accessor. The ~8 desktop consumers stop taking `Arc<store::db::Database>` and instead use the already-managed `SharedStore` plus the new API. The legacy module and the dual-open in `lib.rs` are removed.

**Tech Stack:** Rust, rusqlite (SQLite, FTS5), Tauri v2 + tauri-specta, serde/serde_json.

**Spec:** `specs/2026-06-01-unify-desktop-cli-store-design.md`

---

## File Structure

- `crates/client-core/src/store/schema.rs` — add migration v8 (`settings` table), bump version.
- `crates/client-core/src/store/settings.rs` — **new.** All setting accessors + `SourceSetting`/`SourceAlertSetting` types. One responsibility: persisted app settings.
- `crates/client-core/src/store/mod.rs` — declare `pub mod settings;`.
- Desktop consumers (modify): `clipboard/monitor.rs`, `commands/clips/source_settings.rs`, `commands/clips/retention.rs`, `commands/clips/global_shortcut.rs`, `retention.rs`, `window_manage.rs`, `commands/window.rs`, `commands/auth/deeplink.rs`.
- `apps/desktop/src-tauri/src/lib.rs` — drop legacy `Database` open + `.manage`.
- Delete: `apps/desktop/src-tauri/src/store/db/*`, `apps/desktop/src-tauri/src/store/models.rs`, `mod store;` wiring.
- Retire legacy `LocalClip`: `events.rs`, `sync_status.rs` move to `commands::clips::LocalClip`.

---

## Task 1: client-core schema v8 — `settings` table

**Files:**
- Modify: `crates/client-core/src/store/schema.rs`

- [ ] **Step 1: Write the failing test** (add to the `#[cfg(test)] mod tests` in `schema.rs`)

```rust
#[test]
fn fresh_db_has_settings_table_and_is_v8() {
    let conn = Connection::open_in_memory().unwrap();
    apply_migrations(&conn).unwrap();
    let version: i64 = conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM meta WHERE key='schema_version'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, CURRENT_SCHEMA_VERSION);
    assert_eq!(CURRENT_SCHEMA_VERSION, 8);
    // The settings table exists and round-trips.
    conn.execute(
        "INSERT INTO settings(key, value) VALUES ('k','v')",
        [],
    )
    .unwrap();
    let v: String = conn
        .query_row("SELECT value FROM settings WHERE key='k'", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, "v");
}
```

- [ ] **Step 2: Run it, expect failure**

Run: `cargo test -p cinchcli-core --lib store::schema::tests::fresh_db_has_settings_table_and_is_v8`
Expected: FAIL — `CURRENT_SCHEMA_VERSION` is 7, and `no such table: settings`.

- [ ] **Step 3: Implement migration v8**

In `schema.rs`, change the constant:

```rust
pub const CURRENT_SCHEMA_VERSION: i64 = 8;
```

Add the dispatch after the `current < 7` block in `apply_migrations`:

```rust
    if current < 8 {
        migrate_v8(conn)?;
    }
```

Add the function (next to `migrate_v7`):

```rust
fn migrate_v8(conn: &Connection) -> rusqlite::Result<()> {
    // Generic key/value app settings, shared by the desktop and CLI. Replaces
    // the desktop's separate `com.cinch.app/clips.db` settings table so both
    // front-ends read one store. See store::settings for key conventions.
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
          key   TEXT PRIMARY KEY,
          value TEXT NOT NULL
        );
        UPDATE meta SET value = '8' WHERE key = 'schema_version';
    "#,
    )?;
    Ok(())
}
```

- [ ] **Step 4: Run the test, expect pass**

Run: `cargo test -p cinchcli-core --lib store::schema::`
Expected: PASS (all schema tests).

- [ ] **Step 5: Commit**

```bash
git add crates/client-core/src/store/schema.rs
git commit -m "feat(client-core): add settings table (schema v8)"
```

---

## Task 2: client-core `store::settings` — generic key/value accessors

**Files:**
- Create: `crates/client-core/src/store/settings.rs`
- Modify: `crates/client-core/src/store/mod.rs` (add `pub mod settings;`)

- [ ] **Step 1: Declare the module**

In `crates/client-core/src/store/mod.rs`, add alongside the other `pub mod` lines:

```rust
pub mod settings;
```

- [ ] **Step 2: Write the failing tests** (create `settings.rs` with only tests first)

```rust
//! Local key/value app settings, shared by the desktop and CLI via the single
//! store at `~/.cinch/store.db`. Owns the key conventions and default values
//! for every persisted setting so the desktop carries no setting strings.

use super::{Store, StoreError};
use rusqlite::{params, OptionalExtension};

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
```

- [ ] **Step 3: Run, expect failure**

Run: `cargo test -p cinchcli-core --lib store::settings::`
Expected: FAIL — `get_setting`/`set_setting`/etc. not found.

- [ ] **Step 4: Implement the generic accessors** (insert above the `#[cfg(test)]` block)

```rust
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
```

- [ ] **Step 5: Run, expect pass**

Run: `cargo test -p cinchcli-core --lib store::settings::`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/client-core/src/store/settings.rs crates/client-core/src/store/mod.rs
git commit -m "feat(client-core): add store::settings key/value accessors"
```

---

## Task 3: client-core — typed setting helpers + moved Specta types

**Files:**
- Modify: `crates/client-core/src/store/settings.rs`

- [ ] **Step 1: Write failing tests** (append to the `tests` module in `settings.rs`)

```rust
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
        assert_eq!(global_shortcut(&s).unwrap(), Some("Cmd+Shift+V".to_string()));
    }

    #[test]
    fn excluded_apps_json_round_trip() {
        let s = store();
        assert_eq!(excluded_apps(&s).unwrap(), Vec::<String>::new());
        set_excluded_apps(&s, &["com.1password".to_string(), "com.keychain".to_string()]).unwrap();
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
```

- [ ] **Step 2: Run, expect failure**

Run: `cargo test -p cinchcli-core --lib store::settings::`
Expected: FAIL — helper functions and types not defined.

- [ ] **Step 3: Implement the typed helpers + types** (insert after the generic accessors, before `#[cfg(test)]`)

```rust
#[cfg(feature = "specta")]
use specta::Type;

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
            k.strip_prefix("alert_enabled:").map(|src| SourceAlertSetting {
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
```

- [ ] **Step 4: Run, expect pass**

Run: `cargo test -p cinchcli-core --lib store::settings::`
Expected: PASS. Also `cargo build -p cinchcli-core --features specta` to confirm the Specta derive compiles.

- [ ] **Step 5: Commit**

```bash
git add crates/client-core/src/store/settings.rs
git commit -m "feat(client-core): settings helpers + SourceSetting/SourceAlertSetting types"
```

---

## Task 4: Desktop — source settings + excluded apps onto SharedStore

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands/clips/source_settings.rs`
- Modify: `apps/desktop/src-tauri/src/clipboard/monitor.rs`

- [ ] **Step 1: Read the current files** to see each `Arc<Database>` parameter and `db.get_setting`/`db.set_setting`/`db.is_source_*` call.

Run: `sed -n '1,120p' apps/desktop/src-tauri/src/commands/clips/source_settings.rs`

- [ ] **Step 2: Swap the API in `source_settings.rs`**

For each command: change the state injection from `State<'_, Arc<Database>>` to `State<'_, SharedStore>` (import `crate::SharedStore`; drop `use crate::store::db::Database`). Replace calls:

- `db.get_setting("excluded_apps")` / `db.set_setting("excluded_apps", &json)` → `client_core::store::settings::excluded_apps(&store)` / `set_excluded_apps(&store, &apps)`.
- `db.is_source_auto_copy(src)` / `db.set_source_auto_copy(src, v)` → `client_core::store::settings::is_source_auto_copy(&store, src)` / `set_source_auto_copy(&store, src, v)`.
- `db.get_all_source_settings()` → `client_core::store::settings::all_source_settings(&store)`.
- `db.get_all_source_alert_settings()` → `client_core::store::settings::all_source_alert_settings(&store)`.
- `db.is_source_alert_enabled(src)` / `set_source_alert_enabled` → the `settings::` equivalents.

Replace any `crate::store::db::{SourceSetting, SourceAlertSetting}` references with `client_core::store::settings::{SourceSetting, SourceAlertSetting}`.

- [ ] **Step 3: Swap the API in `monitor.rs`**

`monitor.rs` uses the legacy `Database` only for `excluded_apps`. Remove the `db: Arc<Database>` field/param from the monitor service struct and `load_excluded_apps`; it already holds `store: SharedStore`. Rewrite `load_excluded_apps` to take `&SharedStore` and call `client_core::store::settings::excluded_apps(store)` (and `set_excluded_apps` on the write path). Remove `use crate::store::db::Database`.

- [ ] **Step 4: Update tests** in both files to construct `Arc<client_core::store::Store>` (`Store::open(Path::new(":memory:"))`) instead of `Database`, and assert via the `settings::` API. Delete assertions that depended on the legacy `Database` type.

- [ ] **Step 5: Build + test**

Run: `cargo test -p cinch-desktop-lib --lib commands::clips::source_settings clipboard::monitor`
(Use the desktop lib crate name from `apps/desktop/src-tauri/Cargo.toml` if different.)
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src-tauri/src/commands/clips/source_settings.rs apps/desktop/src-tauri/src/clipboard/monitor.rs
git commit -m "refactor(desktop): source settings + excluded apps via client-core store"
```

---

## Task 5: Desktop — retention onto SharedStore

**Files:**
- Modify: `apps/desktop/src-tauri/src/retention.rs`
- Modify: `apps/desktop/src-tauri/src/commands/clips/retention.rs`
- Modify: `apps/desktop/src-tauri/src/commands/auth/deeplink.rs`

- [ ] **Step 1: Read each file's retention reads/writes.**

Run: `rg -n "get_setting|set_setting|Database|retention" apps/desktop/src-tauri/src/retention.rs apps/desktop/src-tauri/src/commands/clips/retention.rs apps/desktop/src-tauri/src/commands/auth/deeplink.rs`

- [ ] **Step 2: Swap the API**

- `spawn_retention_sweep(db: Arc<Database>)` → `spawn_retention_sweep(store: SharedStore)`; `db.get_setting("local_retention_days")` parsing → `client_core::store::settings::local_retention_days(&store)`.
- `commands/clips/retention.rs`: `db.get_setting("local_retention_days")` / `"remote_retention_days"` and the `set_setting` writes → `settings::local_retention_days` / `remote_retention_days` and the `set_*` setters. Change the command's `State<'_, Arc<Database>>` to `State<'_, SharedStore>`.
- `deeplink.rs`: the `remote_retention_days` read (line ~147) → `settings::remote_retention_days(&store)`, taking `SharedStore` from app state instead of the legacy DB.

- [ ] **Step 3: Update call sites** in `lib.rs`/wherever `spawn_retention_sweep` is invoked to pass the `SharedStore` (will be finalized in Task 8; for now pass `shared_store.clone()`).

- [ ] **Step 4: Update tests** in `commands/clips/retention.rs` to use an in-memory `Store`.

- [ ] **Step 5: Build + test**

Run: `cargo test -p cinch-desktop-lib --lib commands::clips::retention`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src-tauri/src/retention.rs apps/desktop/src-tauri/src/commands/clips/retention.rs apps/desktop/src-tauri/src/commands/auth/deeplink.rs
git commit -m "refactor(desktop): retention settings via client-core store"
```

---

## Task 6: Desktop — shortcuts onto SharedStore

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands/clips/global_shortcut.rs`
- Modify: `apps/desktop/src-tauri/src/window_manage.rs`

- [ ] **Step 1: Read the shortcut reads/writes.**

Run: `rg -n "get_setting|set_setting|global_shortcut|send_shortcut|Database" apps/desktop/src-tauri/src/commands/clips/global_shortcut.rs apps/desktop/src-tauri/src/window_manage.rs`

- [ ] **Step 2: Swap the API**

- `global_shortcut.rs`: `db.get_setting("global_shortcut")` / `db.set_setting("global_shortcut", s)` → `client_core::store::settings::global_shortcut(&store)` / `set_global_shortcut(&store, s)`. Command state → `SharedStore`.
- `window_manage.rs`: the `try_state::<Arc<store::db::Database>>()` lookups for `send_shortcut` and `global_shortcut` → `try_state::<crate::SharedStore>()` and the `settings::send_shortcut` / `settings::global_shortcut` calls. (The window-placement lookup here is handled in Task 7.)

- [ ] **Step 3: Build + test**

Run: `cargo test -p cinch-desktop-lib --lib commands::clips::global_shortcut`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src-tauri/src/commands/clips/global_shortcut.rs apps/desktop/src-tauri/src/window_manage.rs
git commit -m "refactor(desktop): shortcut settings via client-core store"
```

---

## Task 7: Desktop — window placement onto SharedStore

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands/window.rs`
- Modify: `apps/desktop/src-tauri/src/window_manage.rs` (placement lookup)

- [ ] **Step 1: Read `load_placement`/`save_placement` and the `Placement` type + `PLACEMENT_KEY`.**

Run: `rg -n "PLACEMENT_KEY|struct Placement|fn load_placement|fn save_placement|get_setting|set_setting" apps/desktop/src-tauri/src/commands/window.rs`

- [ ] **Step 2: Swap storage, keep the `Placement` struct desktop-side**

Rewrite the two helpers to take `&SharedStore` and serialize the desktop `Placement` to/from the raw string via the client-core key:

```rust
pub fn save_placement(store: &crate::SharedStore, p: &Placement) {
    match serde_json::to_string(p) {
        Ok(json) => {
            if let Err(e) = client_core::store::settings::set_window_placement(store, &json) {
                log::warn!("save_placement failed (non-fatal): {e}");
            }
        }
        Err(e) => log::warn!("save_placement serialize failed: {e}"),
    }
}

pub fn load_placement(store: &crate::SharedStore) -> Option<Placement> {
    let raw = client_core::store::settings::window_placement(store).ok().flatten()?;
    serde_json::from_str(&raw).ok()
}
```

Delete the desktop `PLACEMENT_KEY` constant (now owned by client-core). Update the `window_manage.rs` placement call to fetch `SharedStore` and call `commands::window::load_placement(&store)`.

- [ ] **Step 3: Update the placement test** (`placement_persists_through_settings_store`) to build an in-memory `Store` and assert via `settings::window_placement`.

- [ ] **Step 4: Build + test**

Run: `cargo test -p cinch-desktop-lib --lib commands::window`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/commands/window.rs apps/desktop/src-tauri/src/window_manage.rs
git commit -m "refactor(desktop): window placement via client-core store"
```

---

## Task 8: Desktop — remove legacy store, retire LocalClip, fix lib.rs

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/events.rs`, `apps/desktop/src-tauri/src/sync_status.rs`
- Delete: `apps/desktop/src-tauri/src/store/db/` (all), `apps/desktop/src-tauri/src/store/models.rs`
- Modify: `apps/desktop/src-tauri/src/paths.rs` (remove now-dead legacy paths)

- [ ] **Step 1: Migrate the two `LocalClip` users**

Read `events.rs` and `sync_status.rs`. Replace `use crate::store::models::LocalClip` (and any `store::models::LocalClip`) with `use crate::commands::clips::LocalClip`. Adjust field access to the `commands::clips::LocalClip` shape (it adds `source_app_id`/`source_app`/`source_url`/`sync_state`; the legacy lacked them). For event payloads, construct via the existing `stored_to_local`/`LocalClip::from_legacy` helpers where applicable.

- [ ] **Step 2: Remove the legacy open + manage in `lib.rs`**

Delete:
```rust
let db_path = paths::legacy_db_path();
let db = match store::db::Database::open(&db_path) { ... };
```
and the corresponding `.manage(db.clone())`. Update `spawn_retention_sweep(...)` and any other call sites to pass `shared_store.clone()`. Remove `mod store;` if `store` no longer has any submodule (i.e., after deleting `db` and `models`). If `store/mod.rs` only re-exported `db`/`models`, delete it too.

- [ ] **Step 3: Delete the legacy module files**

```bash
git rm -r apps/desktop/src-tauri/src/store/db
git rm apps/desktop/src-tauri/src/store/models.rs
# remove apps/desktop/src-tauri/src/store/mod.rs if now empty, and `mod store;` in lib.rs
```

Remove `paths::legacy_db_path()` / `app_data_dir()` if no caller remains (grep first).

- [ ] **Step 4: Build the whole desktop crate**

Run: `cargo build -p cinch-desktop-lib`
Expected: compiles; fix any dangling `store::db`/`store::models` references the compiler points to.

- [ ] **Step 5: Commit**

```bash
git add -A apps/desktop/src-tauri/src
git commit -m "refactor(desktop): delete legacy store::db, run solely on the shared store"
```

---

## Task 9: Regenerate bindings, full verification, release note

**Files:**
- Modify: `apps/desktop/src/bindings.ts` (generated), release notes / CHANGELOG.

- [ ] **Step 1: Regenerate Specta bindings**

Run: `cd apps/desktop/src-tauri && cargo test export_bindings -- --ignored`
Expected: `apps/desktop/src/bindings.ts` regenerates; `SourceSetting`/`SourceAlertSetting` still present (same shape, now sourced from client-core). Diff to confirm no unexpected frontend type changes.

- [ ] **Step 2: Full workspace build + tests**

Run: `cargo build --workspace && cargo test -p cinchcli-core --lib && cargo test -p cinch-cli --lib && cargo test -p cinch-desktop-lib --lib`
Expected: all green.

- [ ] **Step 3: Lint**

Run: `cargo clippy -p cinchcli-core --lib && cargo clippy -p cinch-desktop-lib --lib`
Expected: no new warnings in changed files.

- [ ] **Step 4: Add the release note**

Add a line to the desktop changelog / release notes: "Desktop now shares the local store with the CLI. App settings (global shortcut, retention, excluded apps, per-source prefs, window placement) reset to defaults on this update."

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/bindings.ts <changelog>
git commit -m "chore(desktop): regenerate bindings + note settings reset on store unification"
```

---

## Self-Review

**Spec coverage:** v8 settings table (Task 1) ✓; client-core settings module + types (Tasks 2–3) ✓; 8 consumers repointed (Tasks 4–7) ✓; lib.rs dual-open removal + legacy deletion + LocalClip retirement (Task 8) ✓; bindings regen + clean-cutover release note (Task 9) ✓. Non-goals (no import, no retention_prefs/alert_prefs reconciliation, leave `import_legacy_if_present`) are respected — no task touches them.

**Placeholder scan:** client-core code (the genuinely new code) is complete and concrete. Desktop tasks specify exact files, exact old→new API mappings, and a "read first" step because the surrounding code must be edited in place; the function names referenced (`excluded_apps`, `is_source_auto_copy`, `local_retention_days`, `global_shortcut`, `window_placement`, etc.) all exist in Task 2/3.

**Type consistency:** `SourceSetting`/`SourceAlertSetting` fields (`source`, `auto_copy` / `alert_enabled`) match between Task 3 definition and Task 4 usage. `WINDOW_PLACEMENT_KEY` defined in Task 3, used in Task 7. `SharedStore = Arc<client_core::store::Store>` (existing alias) used consistently.

**Note for executor:** confirm the desktop lib crate name in `apps/desktop/src-tauri/Cargo.toml` (used in `cargo test -p <name>`).
