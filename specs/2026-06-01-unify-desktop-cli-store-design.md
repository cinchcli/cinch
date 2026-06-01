# Unify desktop + CLI onto one store (finish Phase 4)

**Date:** 2026-06-01
**Status:** Approved (Approach C, clean cutover)

## Problem

The desktop app opens **two** SQLite databases at startup (`lib.rs`, commented
"Phase 4"):

1. **Legacy desktop DB** — `<data>/com.cinch.app/clips.db`, via
   `apps/desktop/src-tauri/src/store/db/Database`.
2. **Shared store** — `~/.cinch/store.db`, via `client_core::store::Store` —
   the same file the `cinch` CLI uses.

Clips, pinning, media, and source listing already run on the shared store. The
**only** remaining live dependency on the legacy `Database` is the generic
`settings(key, value)` table. This split is half-finished work; it means the
desktop and CLI do not fully share state, and the FTS/index fixes shipped in the
shared store (schema v7) don't benefit anything still reading the legacy DB.

Goal: the desktop and CLI use **one** store (`~/.cinch/store.db`), and the
legacy desktop `store::db` module is deleted.

## Decision: clean cutover

No data is migrated from the legacy DB. On first launch of the consolidated
build, the shared store's `settings` table is empty, so every setting returns
its default. The legacy `com.cinch.app/clips.db` is left untouched on disk
(orphaned), not read.

**User-visible consequence (needs a release note):** existing global shortcut,
send shortcut, local/remote retention days, excluded-apps list, per-source
auto-copy/alert prefs, and saved window placement **reset to defaults** on the
update that ships this change. Pre-Phase-4 legacy clip history that was never
written to the shared store is not carried over.

## Approach C: settings semantics live in client-core

All settings handling — the `settings` table, generic accessors, key
conventions, default values, and the typed domain helpers — moves into
`client-core`. The desktop calls high-level functions and no longer knows key
strings or defaults. The one unavoidable concession: window-placement geometry
types (`Placement`) are GUI-only and stay desktop-side; client-core stores the
placement as a raw JSON string under a key it owns, and the desktop
(de)serializes `Placement` around that raw value.

### 1. client-core schema — migration v8

`crates/client-core/src/store/schema.rs`: bump `CURRENT_SCHEMA_VERSION` to `8`,
add `migrate_v7`-style step:

```sql
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
UPDATE meta SET value = '8' WHERE key = 'schema_version';
```

(The pre-existing `retention_prefs` / `alert_prefs` tables are left as-is — see
Non-goals.)

### 2. client-core settings module

New `crates/client-core/src/store/settings.rs` (declared `pub mod settings;` in
`store/mod.rs`), all functions taking `&Store` and returning
`Result<_, StoreError>`:

- **Generic:** `get_setting(key)`, `set_setting(key, value)`,
  `delete_setting(key)`, `list_settings_with_prefix(prefix) -> Vec<(String, String)>`.
- **Source prefs** (keys + defaults owned here):
  - `is_source_auto_copy(source) -> bool` — key `auto_copy:<source>`, default `false`.
  - `set_source_auto_copy(source, bool)`.
  - `is_source_alert_enabled(source) -> bool` — key `alert_enabled:<source>`, default `true`.
  - `set_source_alert_enabled(source, bool)`.
  - `all_source_settings() -> Vec<SourceSetting>` (scans `auto_copy:%`).
  - `all_source_alert_settings() -> Vec<SourceAlertSetting>` (scans `alert_enabled:%`).
- **Retention** (keys `local_retention_days`, `remote_retention_days`):
  `local_retention_days() -> Option<i64>`, `set_local_retention_days(i64)`,
  and the remote equivalents.
- **Shortcuts** (keys `global_shortcut`, `send_shortcut`):
  `global_shortcut() -> Option<String>`, `set_global_shortcut(&str)`,
  and the send-shortcut equivalents.
- **Excluded apps** (key `excluded_apps`, JSON `Vec<String>` — client-core uses
  `serde_json`, no GUI types): `excluded_apps() -> Vec<String>`,
  `set_excluded_apps(&[String])`.
- **Window placement** (raw passthrough): `window_placement() -> Option<String>`,
  `set_window_placement(&str)`. The key constant lives here; the desktop maps
  the raw string to/from its `Placement` struct.

### 3. Types moved into client-core

`SourceSetting { source, auto_copy }` and `SourceAlertSetting { source, alert_enabled }`
move from `apps/desktop/.../store/db/mod.rs` to `client-core`, deriving
`specta::Type` behind the existing `specta` feature (same pattern as the `Device`
DTO). Names are preserved so the regenerated `bindings.ts` and the frontend
imports are unchanged. Run `cargo test export_bindings -- --ignored` after.

### 4. Desktop consumers repointed (`Arc<store::db::Database>` → `SharedStore`)

Each call site swaps the legacy DB handle for the already-managed
`SharedStore` and calls the client-core settings API:

| File | Settings touched |
|---|---|
| `window_manage.rs` | `send_shortcut`, `global_shortcut`, window placement |
| `commands/window.rs` (`load_placement`/`save_placement`) | window placement (maps raw ↔ `Placement`) |
| `retention.rs` (`spawn_retention_sweep`) | `local_retention_days` |
| `commands/clips/retention.rs` | local/remote retention days |
| `commands/clips/source_settings.rs` | `excluded_apps`, source auto-copy/alert |
| `commands/clips/global_shortcut.rs` | `global_shortcut` |
| `clipboard/monitor.rs` | `excluded_apps` only — drop the `db` field/param |
| `commands/auth/deeplink.rs` | `remote_retention_days` |

### 5. `lib.rs`

Remove `store::db::Database::open(&legacy_db_path)` and `.manage(db.clone())`.
All consumers now run on the `SharedStore` that is already opened and managed.

### 6. Delete the legacy module + retire `LocalClip`

- Delete `apps/desktop/src-tauri/src/store/db/` entirely (`clips.rs`,
  `pinning.rs`, `retention.rs`, `settings.rs`, `sync_queue.rs`, `migrations.rs`,
  `mod.rs`).
- Migrate the two remaining non-db users of the legacy `store::models::LocalClip`
  (`events.rs`, `sync_status.rs`) to `commands::clips::LocalClip`, then remove
  `store/models.rs`, collapsing the desktop `store` module (and its `mod store;`
  wiring) entirely.
- `paths::legacy_db_path()` / `app_data_dir()` become unused → remove if no other
  caller remains.

## Non-goals

- No legacy-data import (clean cutover).
- No reconciliation of client-core's existing `retention_prefs` / `alert_prefs`
  tables with the lifted settings-key model — they are a separate CLI concern.
- The unrelated `import_legacy_if_present` (targets `com.cinchcli.desktop/cinch.db`)
  is left untouched.

## Testing

- **client-core:** unit tests per accessor — defaults when unset, round-trip,
  prefix scans (`all_source_*`), JSON round-trip for `excluded_apps`, window
  placement raw passthrough. Schema test asserts `CURRENT_SCHEMA_VERSION == 8`
  and a fresh DB has the `settings` table.
- **desktop:** rewrite consumer tests to build an in-memory
  `client_core::store::Store` instead of `Database`; delete the legacy `db` tests
  (their behavior is now covered in client-core). Keep `commands/window.rs`'s
  placement-persistence test, retargeted at the shared store.
- Regenerate specta bindings; `cargo build --workspace` + desktop tests green.

## Risks & mitigations

- **Specta type move** — frontend depends on `SourceSetting`/`SourceAlertSetting`.
  Mitigation: preserve field names/shape; regenerate `bindings.ts` and diff.
- **Broad change set** — schema + 8 consumers + deletions. Mitigation: land in the
  ordered steps below; each step compiles and tests green before the next.
- **Concurrent working-tree WIP** (`http/` refactor in flight) is disjoint from
  `store/` + desktop settings; coordinate at merge.

## Implementation ordering (for the plan)

1. client-core: migration v8, `store/settings.rs` API, moved `SourceSetting`/
   `SourceAlertSetting` types, unit tests. (Self-contained; nothing depends on it
   yet.)
2. Desktop: repoint consumers group-by-group onto `SharedStore` + the new API
   (source/excluded-apps, retention, shortcuts, window placement, monitor,
   deeplink), updating each consumer's tests as you go.
3. Remove the `lib.rs` dual-open; delete `store::db`; retire `LocalClip`
   (`events.rs`, `sync_status.rs`) and `store/models.rs`.
4. Regenerate specta bindings; full workspace build + tests; add the release note
   about settings resetting.
