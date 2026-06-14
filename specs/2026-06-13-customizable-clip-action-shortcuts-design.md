# Customizable clip-action shortcuts

**Date:** 2026-06-13
**Status:** Approved design — ready for implementation plan
**Area:** `apps/desktop` (React frontend + Tauri/Rust commands) + `crates/client-core` (settings store)

## Problem

The desktop "Edit clip" shortcut is the **bare `E` key** (`App.tsx`, the
keydown handler). When a clip is selected and the user is not typing in a
field, a single `E` press opens the edit modal — surprising and easy to
trigger by accident. More broadly, every in-app action shortcut is
hardcoded in the `App.tsx` keydown handler with no way for a user to
change it.

The app already ships customizable shortcuts for two **OS-global**
hotkeys (window show/hide and "send current clipboard"), with a proven
DB → Tauri command → React capture-UI pipeline. We extend that same
pattern to the in-app clip-action shortcuts.

## Scope

Make exactly four **in-app action** shortcuts user-configurable in
**Settings → Keyboard**:

| Action | Current binding | New default | Notes |
|---|---|---|---|
| Edit clip | `E` (bare) | **`CmdOrCtrl+E`** | fixes the accidental single-key trigger |
| Copy clip | `Enter` (bare) | `Enter` | bare key kept; `CmdOrCtrl+C` stays a fixed always-on alias |
| Pin / unpin | `CmdOrCtrl+P` | `CmdOrCtrl+P` | unchanged default |
| Send selected | `CmdOrCtrl+Enter` | `CmdOrCtrl+Enter` | unchanged default |

### Out of scope (stay hardcoded)

These remain fixed and are **not** remappable:

- Navigation: `↑`/`↓`, `Ctrl+J`/`Ctrl+K`, `Tab`/`Shift+Tab`, `⌘1`/`⌘2`/`⌘3`
- Search: `⌘F`/`Ctrl+F`, `/`, `Escape`
- Settings: `⌘,`
- Source-filter cycle: `Ctrl+H`/`Ctrl+L`
- Help panel: `?`
- The two existing OS-global hotkeys (`global_shortcut`, `send_shortcut`)
  are untouched by this change.

## Key policy

Modifier is **optional** for these in-app shortcuts (bare keys allowed, so
a user can freely rebind, e.g. set Copy back to `Enter` or Edit to a bare
letter if they insist). Each binding **must contain one real
(non-modifier) key**; empty or modifier-only strings are rejected.

This requires a **new, lighter validator** — we do **not** reuse the
existing global `validate_shortcut()` in
`commands/clips/global_shortcut.rs`, which *mandates* a modifier (correct
for OS-global hotkeys that would otherwise capture all typing, wrong for
in-app keys that only fire when a clip is selected and no field is
focused).

## Architecture & data flow

Mirrors the existing `global_shortcut` plumbing, minus the OS-registration
step (these never register with the OS — they are matched in the JS
keydown handler).

```
~/.cinch/store.db  (settings table, schema v8)
  key "action_shortcuts" → JSON {edit,copy,pin,send}
        │   ← namespaced key; deliberately distinct from the existing
        │     global "send_shortcut" / "global_shortcut" keys
        ▼
  crates/client-core/src/store/settings.rs
    action_shortcuts(store) / set_action_shortcuts(store, json)
        ▼
  apps/desktop/src-tauri/src/commands/clips/action_shortcut.rs
    struct ActionShortcuts { edit, copy, pin, send }   (specta::Type)
    get_action_shortcuts()    → stored merged over defaults
    set_action_shortcuts(s)   → validate all 4, persist JSON
    reset_action_shortcuts()  → delete key, return defaults
        │   tauri-specta → src/bindings.ts  (regenerated; never hand-edited)
        ▼
  apps/desktop/src/App.tsx
    actionShortcuts state, loaded once on mount via getActionShortcuts();
    keydown handler matches events against it.
  apps/desktop/src/components/SettingsPane.tsx
    "Clip actions" capture UI; on save, calls set/reset command AND
    pushes the new value up to App so the live handler updates without restart.
```

### Why a single JSON blob (not four keys)

Persist all four under one settings key `"action_shortcuts"` as a JSON
object (same approach as the existing `excluded_apps` JSON setting). The
getter deserializes and **merges stored values over the defaults**, so a
missing or partial blob still yields a complete, valid set. One typed DTO
(`ActionShortcuts`) crosses the specta boundary — no stringly-typed action
ids, satisfying the "typed interfaces, never `any`" rule.

## Component design

### 1. `crates/client-core/src/store/settings.rs`

Add, in the `── Shortcuts ──` section, alongside `global_shortcut` /
`send_shortcut`:

```rust
pub fn action_shortcuts(store: &Store) -> Result<Option<String>, StoreError> {
    get_setting(store, "action_shortcuts")
}

pub fn set_action_shortcuts(store: &Store, json: &str) -> Result<(), StoreError> {
    set_setting(store, "action_shortcuts", json)
}
```

Storage is a raw JSON string; (de)serialization and defaults live in the
command layer so `client-core` stays free of desktop-specific shapes. No
schema migration — the `settings` table (key/value, schema v8) already
exists.

### 2. `apps/desktop/src-tauri/src/commands/clips/action_shortcut.rs` (new)

```rust
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct ActionShortcuts {
    pub edit: String,   // default "CmdOrCtrl+E"
    pub copy: String,   // default "Enter"
    pub pin: String,    // default "CmdOrCtrl+P"
    pub send: String,   // default "CmdOrCtrl+Enter"
}

impl Default for ActionShortcuts { /* the four defaults above */ }
```

- `get_action_shortcuts(store) -> Result<ActionShortcuts, String>` — read
  the stored JSON; deserialize **field-by-field over `Default`** so a
  missing/partial blob is completed with defaults; return the merged set.
- `set_action_shortcuts(store, shortcuts: ActionShortcuts) -> Result<(), String>`
  — `validate_action_shortcut` each of the four, then persist
  `serde_json::to_string(&shortcuts)`.
- `reset_action_shortcuts(store) -> Result<ActionShortcuts, String>` —
  `delete_setting(store, "action_shortcuts")`, return `ActionShortcuts::default()`.

```rust
/// In-app shortcut validator: modifier OPTIONAL, but a real key is REQUIRED.
/// Rejects empty strings and modifier-only combos (e.g. "CmdOrCtrl+Shift").
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
```

`MODIFIER_NAMES` is the same modifier vocabulary used by
`global_shortcut.rs`; factor it into a shared location (e.g. a small
`shortcut_util` module or re-export) rather than duplicating the slice.

Register the three commands in the specta builder + `invoke_handler` in
`apps/desktop/src-tauri/src/lib.rs` (same place `get/set_global_shortcut`
and `get/set_send_shortcut` are registered), and declare the module in
`commands/clips/mod.rs`.

### 3. `apps/desktop/src/lib/keymap.ts` (new)

Pure, unit-testable matching layer built on the existing
`physicalKey(e)` helper (so matching survives Korean IME / non-QWERTY,
consistent with the rest of the handler).

```ts
export type ActionId = "edit" | "copy" | "pin" | "send";

export const DEFAULT_ACTION_SHORTCUTS = {
  edit: "CmdOrCtrl+E",
  copy: "Enter",
  pin: "CmdOrCtrl+P",
  send: "CmdOrCtrl+Enter",
} as const;

// Parse "CmdOrCtrl+Shift+E" → { primary, shift, alt, key }.
// The key token is normalized to physicalKey's form (single letters/digits
// uppercased, named keys verbatim) so it compares equal to physicalKey(e).
export function parseAccelerator(accel: string): ParsedAccel;

// Exact-modifier match against a live KeyboardEvent.
export function matchesAccelerator(
  e: Pick<KeyboardEvent, "code" | "key" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey">,
  accel: string,
): boolean;
```

**Matching rule (exact modifiers):**
- `primary` (from `Cmd`/`Ctrl`/`Meta`/`CmdOrCtrl`/`CommandOrControl`):
  satisfied iff `metaKey || ctrlKey` is true; if the accelerator has **no**
  primary modifier, **both** `metaKey` and `ctrlKey` must be false.
- `shift`: `e.shiftKey` must equal whether the accelerator names Shift.
- `alt`: `e.altKey` must equal whether the accelerator names Alt/Option.
- key: `physicalKey(e)` must equal the accelerator's key token
  (letters/digits already uppercased by `physicalKey`; named keys like
  `Enter`/`Escape` compared verbatim).

Consequence: bare-`Enter` Copy does **not** fire while `Cmd` is held
(that's the `Cmd+Enter` Send), and `Cmd+E` does not fire on `Cmd+Shift+E`.

```ts
// Conflict detection for the Settings UI.
export function findConflict(next: Record<ActionId, string>): ConflictResult;
```

`findConflict` blocks:
1. **Duplicate within the four** — two actions resolving to the same
   normalized accelerator.
2. **Reserved collisions** — a curated `RESERVED_ACCELERATORS` set
   covering the fixed in-app keys that would clash: the `CmdOrCtrl+C`
   copy alias, navigation (`ArrowUp`/`ArrowDown`, `Ctrl+J`/`Ctrl+K`,
   `Tab`), search (`CmdOrCtrl+F`, `/`, `Escape`), settings (`CmdOrCtrl+,`),
   source cycle (`Ctrl+H`/`Ctrl+L`), help (`?`), and panel jumps
   (`CmdOrCtrl+1..3`).

On any conflict the Settings UI shows an inline error and **does not
persist** — nothing is saved until the binding is unique and unreserved.

### 4. `apps/desktop/src/App.tsx`

- Add `const [actionShortcuts, setActionShortcuts] = useState<ActionShortcuts>(DEFAULT_ACTION_SHORTCUTS)`.
- Load once on mount: `unwrap(commands.getActionShortcuts()).then(setActionShortcuts)` (fall back to defaults on error, same as `getGlobalShortcut`).
- In the keydown handler, replace the four hardcoded branches with
  `matchesAccelerator` checks, preserving every existing guard:
  - **Edit:** `matchesAccelerator(e, actionShortcuts.edit) && !isTextEntry && selectedClip && selectedClip.content_type !== "image"`.
  - **Pin:** `matchesAccelerator(e, actionShortcuts.pin)` — keep the
    unconditional `preventDefault()` (blocks the webview print dialog when
    the binding is still `Cmd+P`), then act only when `selectedClip`.
  - **Send:** `matchesAccelerator(e, actionShortcuts.send) && selectedClip`
    — evaluated **before** Copy (preserve current ordering).
  - **Copy:** `matchesAccelerator(e, actionShortcuts.copy) && selectedClip && (!isTextEntry || e.target === searchRef.current)`; keep the separate
    fixed `Cmd+C`-when-no-text-selection alias and the Enter-in-search affordance.
- Add `actionShortcuts` to the effect's dependency array.
- Pass `actionShortcuts` and a setter (or change handler) down to `SettingsPane` so an edit in Settings updates the live handler without an app restart.

### 5. `apps/desktop/src/components/SettingsPane.tsx`

- New **"Clip actions"** subsection in the Keyboard tab, below the
  existing Launch/Send shortcut rows.
- One row per action (Edit / Copy / Pin / Send): label + current binding
  rendered via the existing `formatShortcutDisplay` + a click-to-capture
  input. Capture reuses the `physicalKey`-based pattern of the existing
  `handleShortcutKeyDown`, but with the **modifier-optional** rule, and
  runs `findConflict` before saving.
- One **"Reset to defaults"** button → `commands.resetActionShortcuts()`
  → update local + App state.
- On save: call `commands.setActionShortcuts(next)`, then propagate the
  new set up to `App` via the passed handler. On conflict/validation
  error, show the inline message and persist nothing.
- Make the four entries in the static "built-in shortcuts" reference list
  reflect the live bindings, and **remove the misleading `⌘⌫ Delete`
  entry** (that shortcut is listed today but is not implemented in the
  handler).

## Testing

### Rust (`commands/clips/action_shortcut.rs`, `#[cfg(test)]`)
- defaults returned when the key is missing.
- roundtrip: set then get returns the same four values.
- partial/legacy JSON blob is completed with defaults (merge behavior).
- `validate_action_shortcut` **accepts** bare `"Enter"` and `"E"`.
- `validate_action_shortcut` **rejects** `""` and modifier-only `"CmdOrCtrl+Shift"`.
- `reset_action_shortcuts` clears the key and returns defaults.

### TypeScript (`src/lib/keymap.test.ts`, new)
- `matchesAccelerator`: `Cmd+E` matches `{metaKey,E}` but not `{metaKey,shiftKey,E}`; bare `Enter` matches `{Enter}` but not `{metaKey,Enter}`; `CmdOrCtrl+P` matches both `{metaKey,P}` and `{ctrlKey,P}`.
- `parseAccelerator` round-trips the accelerator formats in use.
- `findConflict`: flags duplicate-within-four and a reserved collision (e.g. setting Edit to `CmdOrCtrl+C`); passes a clean set.

### Build / verify
- Regenerate bindings: `cd apps/desktop/src-tauri && cargo test export_bindings -- --ignored`.
- `make test` (cargo workspace + desktop) and `make lint` from `cinch/main/`.

## Risks & non-goals

- **No OS registration** — these are JS-handler shortcuts only; setting one
  never calls the global-shortcut plugin, so there is no cross-app
  conflict surface.
- **No new settings migration** — reuses the existing key/value `settings`
  table (schema v8).
- **Navigation/search/global remapping is explicitly out of scope** — the
  reserved set guards against colliding *into* those keys, but they
  themselves stay fixed.
- Per-row reset is intentionally **not** included — a single
  "Reset to defaults" matches the approved mockup and keeps the UI simple.
```

