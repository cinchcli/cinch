# Background Menu-Bar Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Cinch macOS desktop app a background menu-bar agent — hidden from the Dock and Cmd+Tab switcher, kept alive when its window is "quit", summoned via the existing Cmd+Shift+W hotkey and the existing tray icon.

**Architecture:** Two surgical edits on top of the existing tray/window plumbing. (1) Switch the macOS activation policy to `Accessory` in `.setup()`. (2) Extend the already-present `ExitRequested { code: None }` arm so Cmd+Q hides the window instead of leaving it on screen. The tray's "Quit Cinch" (`app.exit(0)` → `code: Some(0)`) already passes through and is the only real quit.

**Tech Stack:** Rust, Tauri v2 (2.11.2), macOS (`objc`), `log`.

**Spec:** `specs/2026-05-24-background-menubar-agent-design.md`

**Baseline:** branch `agent/claude/tray-menu-redesign` @ `7ae9cd4` (spec commit) / Rust baseline `e3ce9df`.

---

## Why there are no new unit tests

Both changes are thin macOS side effects: one FFI call (`set_activation_policy`) and one window-hide loop inside an event closure. There is no pure, branchable logic to assert on — mirroring the spec's "Testing" section and the repo's manual-testing convention. The automated gate is therefore "it compiles, clippy is clean, and the existing suite still passes"; correctness is confirmed by the manual checklist in Task 3. Do **not** invent fake unit tests for the FFI calls.

All `cargo` commands run from the workspace root:
`/Users/jinmu/Programming/cinchcli/cinch/claude-tray-menu-redesign`
The crate is `cinch-desktop`. `npm` commands run from `apps/desktop`.

---

## Task 1: Become a background agent (Accessory activation policy)

**Files:**
- Modify: `apps/desktop/src-tauri/src/window_manage.rs` (add helper after `configure_macos_window`, ~line 159)
- Modify: `apps/desktop/src-tauri/src/lib.rs` (call helper in `.setup()`, after `configure_macos_window(handle)` ~line 388)

- [ ] **Step 1: Add the `configure_activation_policy` helper**

In `apps/desktop/src-tauri/src/window_manage.rs`, find the existing pair of `configure_macos_window` definitions (the macOS impl ends ~line 156, followed by the `#[cfg(not(target_os = "macos"))]` no-op stub ~line 159). Add this new pair immediately after the no-op stub:

```rust
/// Make the app a background menu-bar agent on macOS: no Dock icon, hidden
/// from the Cmd+Tab app switcher, and no top-left app menu. Only the tray
/// status icon remains.
///
/// Tauri keeps the default `NSApp.mainMenu` even though it is no longer drawn
/// for an Accessory app, so its key equivalents still fire: Cmd+C/V/X/A keep
/// working in text fields and Cmd+Q still routes through
/// `RunEvent::ExitRequested` (which `lib.rs` turns into a window hide).
#[cfg(target_os = "macos")]
pub(crate) fn configure_activation_policy(app: &tauri::AppHandle) {
    if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Accessory) {
        log::warn!("failed to set Accessory activation policy: {}", e);
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn configure_activation_policy(_app: &tauri::AppHandle) {}
```

(`AppHandle::set_activation_policy(&self, ActivationPolicy) -> tauri::Result<()>` is macOS-only in Tauri 2.11.2; `ActivationPolicy` is re-exported at `tauri::ActivationPolicy`.)

- [ ] **Step 2: Call the helper in `.setup()`**

In `apps/desktop/src-tauri/src/lib.rs`, find this block inside `.setup()` (~line 385-388):

```rust
            // Make the window movable by external window managers (Rectangle, Moom, etc.).
            // decorations:false sets NSWindowStyleMaskBorderless whose default is isMovable=false,
            // so Rectangle's AX-based "Move to Next Display" silently fails.
            window_manage::configure_macos_window(handle);
```

Insert the new call immediately after it:

```rust
            // Run as a background menu-bar agent: no Dock icon, hidden from the
            // Cmd+Tab switcher, no top-left app menu. The tray status icon stays.
            window_manage::configure_activation_policy(handle);
```

(`handle` is `&AppHandle` here — the same value already passed to `configure_macos_window`.)

- [ ] **Step 3: Build**

Run: `cargo build -p cinch-desktop`
Expected: builds successfully, no errors.

- [ ] **Step 4: Lint**

Run: `cargo clippy -p cinch-desktop --all-targets`
Expected: no warnings from the new code.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/window_manage.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "app: run as background menu-bar agent (Accessory activation policy)"
```

---

## Task 2: Cmd+Q hides the window instead of leaving it on screen

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs` (the `.run(...)` callback, ~line 519-529)

- [ ] **Step 1: Extend the `ExitRequested` handler**

In `apps/desktop/src-tauri/src/lib.rs`, find the final `.run(...)` callback (~line 519):

```rust
        .run(|_app, event| {
            // Keep the app alive when macOS fires an implicit ExitRequested
            // (e.g., last window closed). Explicit `app.exit(n)` calls — including
            // the tray's "Quit Cinch" — set `code = Some(n)`, so they pass through.
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
            }
        });
```

Replace it with:

```rust
        .run(|app, event| {
            // Keep the app alive when macOS fires an implicit ExitRequested
            // (e.g., Cmd+Q, or the last window closed). Explicit `app.exit(n)`
            // calls — including the tray's "Quit Cinch" — set `code = Some(n)`,
            // so they pass through and terminate the process.
            if let tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } = event
            {
                api.prevent_exit();
                // Tuck the window away so Cmd+Q behaves like Cmd+W: the app
                // lives on in the menu bar and is re-summoned via the hotkey.
                for window in app.webview_windows().values() {
                    let _ = window.hide();
                }
            }
        });
```

(The only changes: `_app` → `app`, and the `for` loop. `webview_windows()` comes from `tauri::Manager`, already imported at `lib.rs:29`.)

- [ ] **Step 2: Build**

Run: `cargo build -p cinch-desktop`
Expected: builds successfully, no errors.

- [ ] **Step 3: Lint**

Run: `cargo clippy -p cinch-desktop --all-targets`
Expected: no warnings from the changed closure.

- [ ] **Step 4: Run existing tests (no regressions)**

Run: `cargo test -p cinch-desktop`
Expected: PASS (including the existing `tray::tests::status_label_*` tests). No new tests are expected here — see "Why there are no new unit tests".

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src-tauri/src/lib.rs
git commit -m "app: hide window on Cmd+Q so it stays alive in the menu bar"
```

---

## Task 3: Manual verification (no code change, no commit)

The Dock / Cmd+Tab / hotkey behaviors only manifest in a real app run, so verify by hand. Run a dev build and walk the checklist.

**Files:** none.

- [ ] **Step 1: Launch a dev build**

From `apps/desktop`:
Run: `npm run tauri dev`
Expected: app launches; the dashboard window appears on launch (unchanged "always show on launch" behavior); a menu-bar status icon is present at the top-right.

- [ ] **Step 2: Verify it is a background agent**

- App has **no Dock icon**.
- Cmd+Tab does **not** list Cinch in the app switcher.
- The top-left app menu (Apple-logo strip with app name / File / Edit / …) does **not** appear for Cinch when its window is focused — only the menu-bar status icon represents the app.

- [ ] **Step 3: Verify the hotkey + text input**

- Press **Cmd+Shift+W** → the dashboard appears, focused, on the monitor under the cursor.
- Click into the search field; type; confirm **Cmd+A / Cmd+C / Cmd+V / Cmd+X** work (proves the default main menu's key equivalents survived the Accessory switch).

- [ ] **Step 4: Verify Cmd+Q behavior**

- With the window focused, press **Cmd+Q** → the window hides; the app keeps running (menu-bar icon still present; process not terminated).
- Press **Cmd+Shift+W** again → the window re-appears. (Repeat once to confirm it is reliably re-summonable.)
- Also confirm the close box / **Cmd+W** still hides (unchanged).

- [ ] **Step 5: Verify the real quit path**

- Click the menu-bar icon → **Quit Cinch** → the process actually terminates (menu-bar icon disappears; `npm run tauri dev` reports the app exited).

- [ ] **Step 6: Stop the dev build**

If still running, stop the dev process (Ctrl+C in the `npm run tauri dev` terminal).

---

## Self-Review (completed during planning)

- **Spec coverage:**
  - "Drop out of Dock + Cmd+Tab (Accessory)" → Task 1. ✓
  - "Cmd+Q → also hide the window" → Task 2. ✓
  - "Global hotkey + menu icon already done" → no task needed; verified in Task 3 Steps 3 & 1. ✓
  - "Launch behavior unchanged" → no task; explicitly verified in Task 3 Step 1. ✓
  - Spec testing checklist → Task 3 Steps 2-5. ✓
- **Placeholder scan:** none — every code step shows complete code; every run step shows the command and expected result.
- **Type consistency:** `configure_activation_policy(app: &tauri::AppHandle)` defined in Task 1, called with `handle` (a `&AppHandle`) in Task 1 Step 2. `tauri::ActivationPolicy::Accessory`, `tauri::RunEvent::ExitRequested { code: None, api, .. }`, and `app.webview_windows()` / `window.hide()` all match the verified Tauri 2.11.2 signatures.
- **Out-of-scope guardrails honored:** no `tray.rs`, `app_state.rs`, Info.plist, global-shortcut-value, or launch-behavior changes.
