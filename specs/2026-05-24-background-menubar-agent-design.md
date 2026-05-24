# Background menu-bar agent (macOS)

**Date:** 2026-05-24
**Component:** `apps/desktop` (Tauri v2, macOS)
**Status:** Approved design — ready for implementation plan
**Baseline:** branch `agent/claude/tray-menu-redesign` @ `e3ce9df`

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
- **Quit disambiguation already exists** (commit `e3ce9df`): the `.run`
  `ExitRequested` handler only calls `api.prevent_exit()` for **implicit**
  exits (`code: None` — last window closed, OS terminate). Explicit
  `app.exit(n)` calls — the tray's "Quit Cinch" → `app.exit(0)` → `code:
  Some(0)` — pass straight through and terminate. So the tray quit works,
  and Cmd+Q (an OS terminate, `code: None`) is **already prevented** today.

## Decisions (from brainstorming)

1. **App presence:** Pure menu-bar agent. Accept the loss of the top-left
   app menu bar that comes with the Accessory activation policy. Only the
   top-right status icon remains.
2. **Cmd+Q:** Hide the window and keep the app running. The only real quit
   is the tray's "Quit Cinch".
3. **Launch behavior:** Unchanged — always show the dashboard on launch.

## The gap → two surgical changes

### 1. Drop out of Dock + Cmd+Tab — `ActivationPolicy::Accessory`

The app currently uses macOS's default `.regular` activation policy, so it
shows a Dock icon and appears in Cmd+Tab. Switch to Accessory.

- Add a helper `configure_activation_policy(app)` in `window_manage.rs`,
  mirroring the existing `configure_macos_window` factoring:
  - macOS: set `ActivationPolicy::Accessory`.
  - non-macOS: no-op stub (`#[cfg(not(target_os = "macos"))]`).
- Call it from `lib.rs` `.setup()`, right next to `configure_macos_window`
  (after `setup_tray`, around lib.rs:388).
- **Verify the receiver during implementation:** `set_activation_policy`
  is macOS-only. Prefer calling it on `&AppHandle` (to match
  `configure_macos_window(app: &tauri::AppHandle)`); if it is only exposed
  on `&App`/`&mut App` in this Tauri version, call it directly on the
  `app` binding inside `.setup()` instead.

This removes the Dock icon, hides the app from Cmd+Tab, and drops the
top-left app menu. The tray status icon is unaffected.

**Why text input still works:** Tauri keeps a default `NSApp.mainMenu`
even when it is not drawn for an Accessory app. Its key equivalents are
still processed via the responder chain, so Cmd+C/V/X/A keep working in
the dashboard's text fields, and **Cmd+Q still routes through
`RunEvent::ExitRequested`** — the basis for change 2. No menu rebuild
needed.

### 2. Cmd+Q → also hide the window (extend the existing handler)

Today the `code: None` arm only calls `api.prevent_exit()`, so Cmd+Q keeps
the app alive but leaves the window on screen. Make it tuck the window
away, matching how Cmd+W already behaves.

- In the `.run(|_app, event| …)` callback (lib.rs:519): rebind `_app` →
  `app`, and in the existing `ExitRequested { code: None, api, .. }` arm,
  in addition to `api.prevent_exit()`, hide every webview window
  (`for w in app.webview_windows().values() { let _ = w.hide(); }`).
- **No new state, no `tray.rs` change.** The pre-existing `code:
  None`/`Some` split already disambiguates Cmd+Q (hide) from the tray's
  explicit `app.exit(0)` (quit), so the earlier `QuitFlag` idea is dropped
  as redundant.

### 3. Global hotkey + menu icon — already done

No change. `register_global_shortcuts` already defaults to
`CmdOrCtrl+Shift+W` → `show_on_active_monitor`, and the tray icon already
exists. Listed only to record that these requirements are met.

## Data flow

```
Cmd+Q  → NSApp terminate: → RunEvent::ExitRequested { code: None }
           └─ prevent_exit() + hide all windows                    (stays alive)

Tray "Quit Cinch" → app.exit(0) → ExitRequested { code: Some(0) }
           └─ (not matched, not prevented) → process exits

Cmd+Shift+W → global shortcut → show_on_active_monitor
           └─ activate_self (activateIgnoringOtherApps) + show + set_focus

Close box / Cmd+W → CloseRequested → prevent_close + hide          (unchanged)
```

## Components touched

| File | Change |
| --- | --- |
| `src-tauri/src/window_manage.rs` | Add `configure_activation_policy` (macOS → Accessory; non-macOS no-op). |
| `src-tauri/src/lib.rs` | Call `configure_activation_policy` in `.setup()`; extend the `ExitRequested { code: None }` arm to also hide all webview windows (rebind `_app` → `app`). |

`tray.rs` and `app_state.rs` are **not** touched.

## Out of scope (YAGNI)

- No launch-behavior change (always show on launch was chosen).
- No Info.plist `LSUIElement`. The runtime `set_activation_policy` is
  enough; since the window shows on launch anyway, a momentary Dock flash
  is moot. (Revisit only if a launch flash is observed and disliked.)
- No change to the global-shortcut value, the tray menu, or the existing
  quit disambiguation.

## Testing

Both changes are thin macOS side effects (an activation-policy call and a
window-hide loop), so there is no meaningful pure logic to unit-test.
Verify manually:

1. App is absent from Cmd+Tab and the Dock; menu-bar status icon present
   and its menu works.
2. Cmd+Shift+W shows the dashboard; copy / paste / select-all work in its
   text fields (confirms the default main menu's key equivalents survive
   the Accessory switch).
3. Cmd+Q hides the window; the app survives (menu-bar icon stays); the
   hotkey re-summons it.
4. Tray "Quit Cinch" actually terminates the process.

## Risks / notes

- If Cmd+Q does not reach `ExitRequested` on some macOS version for an
  Accessory app, the fallback is to keep/install an explicit app menu with
  a Quit key-equivalent we intercept. Expected unnecessary because Tauri
  retains the default main menu.
- Accessory-app window focus depends on `activateIgnoringOtherApps`,
  already called in `show_on_active_monitor`. If focus ever fails after the
  policy switch, that call site is where to look.
- Pre-existing: the repo's `lefthook` pre-commit runs `rust-fmt` across the
  crate, so a commit touching only docs can still trip on unrelated Rust
  formatting. Implementation must leave `cargo fmt` clean.
