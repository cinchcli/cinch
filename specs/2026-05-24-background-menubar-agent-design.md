# Background menu-bar agent (macOS)

**Date:** 2026-05-24
**Component:** `apps/desktop` (Tauri v2, macOS)
**Status:** Approved design — ready for implementation plan

## Goal

Make the Cinch desktop app behave like Docker Desktop / Rectangle: a
background **menu-bar agent** that is not visible in the Cmd+Tab app
switcher or the Dock, stays running when the user "quits" its window, and
is summoned with a global hotkey.

## Background: what already exists

Most of the desired behavior is already implemented and needs **no change**:

- Window starts hidden — `tauri.conf.json` window has `"visible": false`,
  borderless, transparent.
- Closing the window (Cmd+W / close box) is intercepted and hides instead
  of closing — `lib.rs` `on_window_event` → `CloseRequested` →
  `prevent_close()` + `window.hide()`.
- A menu-bar status icon already exists — `tray.rs` (`setup_tray`), with
  status row, Open Dashboard, Settings, Check for Updates, Quit.
- A global shortcut is already registered and defaults to exactly
  `CmdOrCtrl+Shift+W` → `show_on_active_monitor` — `window_manage.rs`
  (`register_global_shortcuts`).
- `show_on_active_monitor` already calls `activate_self()`
  (`activateIgnoringOtherApps`), which is what lets the window come to the
  front and take keyboard focus.

## Decisions (from brainstorming)

1. **App presence:** Pure menu-bar agent. Accept the loss of the top-left
   app menu bar that comes with the Accessory activation policy. Only the
   top-right status icon remains.
2. **Cmd+Q:** Hide the window and keep the app running. The only real quit
   is the tray's "Quit Cinch".
3. **Launch behavior:** Unchanged — always show the dashboard on launch.

## The gap → three surgical changes

### 1. Drop out of Dock + Cmd+Tab — `ActivationPolicy::Accessory`

The app currently uses macOS's default `.regular` activation policy, so it
shows a Dock icon and appears in Cmd+Tab. Switch to Accessory.

- Add a helper `configure_activation_policy(app)` in `window_manage.rs`,
  mirroring the existing `configure_macos_window` factoring:
  - macOS: `app.set_activation_policy(tauri::ActivationPolicy::Accessory)`.
  - non-macOS: no-op stub (`#[cfg(not(target_os = "macos"))]`).
- Call it from `lib.rs` `.setup()` (near `configure_macos_window` /
  `setup_tray`).

This removes the Dock icon, hides the app from Cmd+Tab, and drops the
top-left app menu. The tray status icon is unaffected.

**Why text input still works:** Tauri keeps a default `NSApp.mainMenu`
even when it is not drawn for an Accessory app. Its key equivalents are
still processed via the responder chain, so Cmd+C/V/X/A keep working in
the dashboard's text fields, and **Cmd+Q still routes through
`RunEvent::ExitRequested`** — the basis for change 2. No menu rebuild
needed.

### 2. Cmd+Q → hide window, keep running (explicit quit flag)

The trap: the tray's "Quit Cinch" calls `app.exit(0)`, which also surfaces
as `ExitRequested`. Blindly calling `api.prevent_exit()` would break the
only real quit path. Disambiguate with an explicit intent flag.

- New managed state `QuitFlag(Arc<AtomicBool>)` — a **newtype**, because a
  bare `Arc<AtomicBool>` is already managed (`relay_connected`) and Tauri
  keys managed state by type. Defaults to `false`.
- Tray `"quit"` handler (`tray.rs`): set the flag to `true`, **then**
  `app.exit(0)`.
- `ExitRequested` handler (`lib.rs`, the `.run(|app, event| …)` callback):
  - read `QuitFlag`;
  - if `false` → `api.prevent_exit()` and hide every webview window (so
    Cmd+Q tucks the window away, matching how Cmd+W already behaves);
  - if `true` → do nothing, let the process exit.
- The `.run` callback currently binds `_app`; rebind to `app` to reach
  managed state and the windows.

This makes "real quit" vs "Cmd+Q hide" explicit and independent of Tauri's
internal exit semantics.

### 3. Global hotkey + menu icon — already done

No change. `register_global_shortcuts` already defaults to
`CmdOrCtrl+Shift+W` → `show_on_active_monitor`, and the tray icon already
exists. Listed here only to record that these requirements are met by
existing code.

## Data flow

```
Cmd+Q  → NSApp terminate: → RunEvent::ExitRequested
           └─ QuitFlag false → prevent_exit() + hide all windows   (stays alive)

Tray "Quit Cinch" → QuitFlag = true → app.exit(0) → ExitRequested
           └─ QuitFlag true → (no prevent) → process exits

Cmd+Shift+W → global shortcut → show_on_active_monitor
           └─ activate_self (activateIgnoringOtherApps) + show + set_focus

Close box / Cmd+W → CloseRequested → prevent_close + hide           (unchanged)
```

## Components touched

| File | Change |
| --- | --- |
| `src-tauri/src/window_manage.rs` | Add `configure_activation_policy` (macOS sets Accessory; non-macOS no-op). |
| `src-tauri/src/lib.rs` | Manage `QuitFlag`; call `configure_activation_policy` in `.setup()`; rewrite the `ExitRequested` arm to gate on the flag and hide windows. |
| `src-tauri/src/tray.rs` | `"quit"` handler sets `QuitFlag = true` before `app.exit(0)`. |
| `src-tauri/src/app_state.rs` | Define `QuitFlag(Arc<AtomicBool>)` newtype next to the other shared handles (`ClipNotifierTx`, `WriterHandle`, …), re-exported from `lib.rs`. |

## Out of scope (YAGNI)

- No launch-behavior change (always show on launch was chosen).
- No Info.plist `LSUIElement`. The runtime `set_activation_policy` is
  enough; since the window shows on launch anyway, a momentary Dock flash
  is moot. (Revisit only if a launch flash is observed and disliked.)
- No change to the global-shortcut value or the tray menu contents.

## Testing

- **Unit:** the only pure logic is the quit-flag gate. Keep the
  `ExitRequested` arm thin; if it helps readability/testability, extract
  `fn should_prevent_exit(quit_requested: bool) -> bool` and unit-test it.
  Activation policy and window hiding are macOS side effects, verified
  manually.
- **Manual checklist:**
  1. App is absent from Cmd+Tab and the Dock.
  2. Menu-bar status icon is present and its menu works.
  3. Cmd+Shift+W shows the dashboard; copy/paste/select-all work in its
     text fields.
  4. Cmd+Q hides the window; the app survives (menu-bar icon stays); the
     hotkey re-summons it.
  5. Tray "Quit Cinch" actually terminates the process.

## Risks / notes

- If Cmd+Q does not reach `ExitRequested` on some macOS version for an
  Accessory app, the fallback is to install/keep an explicit app menu with
  a Quit key-equivalent we intercept. Expected to be unnecessary because
  Tauri retains the default main menu.
- Accessory-app window focus depends on `activateIgnoringOtherApps`, which
  is already called in `show_on_active_monitor`. If focus ever fails after
  the policy switch, that call site is where to look.
