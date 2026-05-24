# Background menu-bar agent (macOS)

**Date:** 2026-05-24
**Component:** `apps/desktop` (Tauri v2, macOS)
**Status:** Implemented & manually verified
**Baseline:** branch `agent/claude/tray-menu-redesign` @ `e3ce9df`

> **Revision (post-implementation):** Change 2 originally extended the
> `ExitRequested { code: None }` arm to hide windows. Manual testing showed
> Cmd+Q still quit instantly. Root cause (verified in source): Tauri's default
> macOS menu makes Quit a *predefined* item whose action is the native
> `terminate:` selector (Cmd+Q), and tao installs no `applicationShouldTerminate:`
> override — so `[NSApp terminate:]` kills the process directly and never emits a
> preventable `ExitRequested`. The fix replaces the default menu with a custom one
> (`app_menu.rs`) whose Cmd+Q slot is a plain `MenuItem` routed through
> `on_menu_event` → window hide. Change 2 below reflects the corrected approach.

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

### 2. Cmd+Q → hide the window (own the menu item)

`ExitRequested` cannot intercept Cmd+Q: the default menu's predefined Quit
fires the native `terminate:` selector, which tao does not cancel, so the
process dies before any preventable event. Instead, **own the Cmd+Q menu
item** so it fires a Tauri menu event we handle.

- Add `app_menu.rs` with:
  - `HIDE_WINDOW_ID` constant.
  - `build_menu(app) -> Menu<Wry>` — builds a custom menu replacing the
    Tauri default: an **App** submenu whose Cmd+Q slot is a plain
    `MenuItem` (id `HIDE_WINDOW_ID`, accelerator `CmdOrCtrl+Q`) instead of
    `PredefinedMenuItem::quit`; an **Edit** submenu (undo/redo/cut/copy/
    paste/select-all) and a **Window** submenu (minimize/close-window) so
    the shortcuts the default menu provided keep working after replacement.
  - `handle_menu_event(app, event)` — on `HIDE_WINDOW_ID`, hide every
    webview window.
- Wire it on the builder in `lib.rs`: `.menu(app_menu::build_menu)` and
  `.on_menu_event(app_menu::handle_menu_event)`.
- The `.run` `ExitRequested { code: None }` arm stays at `prevent_exit()`
  only (its `e3ce9df` form) — it remains the safety net for implicit exits;
  Cmd+Q no longer flows through it.
- **No new state, no `tray.rs` change.** The tray's `app.exit(0)` (real
  quit) and the menu's Cmd+Q (hide) are now cleanly separate code paths.

**Why the hidden menu still works:** for an Accessory app the menu bar is
not drawn, but its key equivalents are still processed while a Cinch window
is focused — the same mechanism the Edit shortcuts rely on. So the custom
Cmd+Q item fires even though the menu is invisible. (Verified manually.)

### 3. Global hotkey + menu icon — already done

No change. `register_global_shortcuts` already defaults to
`CmdOrCtrl+Shift+W` → `show_on_active_monitor`, and the tray icon already
exists. Listed only to record that these requirements are met.

## Data flow

```
Cmd+Q  → custom menu item (HIDE_WINDOW_ID) → on_menu_event
           └─ hide all webview windows                             (stays alive)

Tray "Quit Cinch" → app.exit(0) → ExitRequested { code: Some(0) }
           └─ (not matched, not prevented) → process exits

Cmd+Shift+W → global shortcut → show_on_active_monitor
           └─ activate_self (activateIgnoringOtherApps) + show + set_focus

Close box / Cmd+W → CloseRequested → prevent_close + hide          (unchanged)

Implicit exit (e.g. last window closed) → ExitRequested { code: None }
           └─ prevent_exit()  (safety net; not hit in normal use)
```

## Components touched

| File | Change |
| --- | --- |
| `src-tauri/src/window_manage.rs` | Add `configure_activation_policy` (macOS → Accessory; non-macOS no-op). |
| `src-tauri/src/app_menu.rs` | **New.** Custom menu (`build_menu`) replacing the default, with a plain Cmd+Q item; `handle_menu_event` hides windows; `HIDE_WINDOW_ID`. |
| `src-tauri/src/lib.rs` | `mod app_menu;`; call `configure_activation_policy` in `.setup()`; `.menu(app_menu::build_menu)` + `.on_menu_event(app_menu::handle_menu_event)` on the builder. (`ExitRequested { code: None }` arm left at `prevent_exit()` only.) |

`tray.rs` and `app_state.rs` are **not** touched.

## Out of scope (YAGNI)

- No launch-behavior change (always show on launch was chosen).
- No Info.plist `LSUIElement`. The runtime `set_activation_policy` is
  enough; since the window shows on launch anyway, a momentary Dock flash
  is moot. (Revisit only if a launch flash is observed and disliked.)
- No change to the global-shortcut value, the tray menu, or the existing
  quit disambiguation.

## Testing

The changes are macOS side effects (an activation-policy call, a custom
menu, a window-hide handler) with no pure logic to unit-test. Verified
manually (all passed):

1. App is absent from Cmd+Tab and the Dock; menu-bar status icon present
   and its menu works.
2. Cmd+Shift+W shows the dashboard; copy / paste / select-all work in its
   text fields (confirms the custom menu's key equivalents fire while the
   menu bar is hidden under Accessory).
3. Cmd+Q hides the window; the app survives (menu-bar icon stays); the
   hotkey re-summons it.
4. Tray "Quit Cinch" actually terminates the process.

## Risks / notes

- **Resolved during implementation:** Cmd+Q via the default menu's
  predefined Quit uses the native `terminate:` selector, which tao does not
  cancel, so it bypasses `ExitRequested` and kills the process. Fixed by
  owning the menu (custom plain Cmd+Q item → `on_menu_event` → hide). If a
  future macOS/Tauri version stops delivering the custom item's key
  equivalent for an Accessory app, the fallback is an `applicationShouldTerminate:`
  override via objc.
- Replacing the default menu means new standard shortcuts are NOT inherited
  automatically — if more are needed later, add them to `app_menu.rs`.
- Accessory-app window focus depends on `activateIgnoringOtherApps`,
  already called in `show_on_active_monitor`. If focus ever fails after the
  policy switch, that call site is where to look.
- Pre-existing: the repo's `lefthook` pre-commit runs `rust-fmt` across the
  crate, so a commit touching only docs can still trip on unrelated Rust
  formatting. Implementation must leave `cargo fmt` clean.
