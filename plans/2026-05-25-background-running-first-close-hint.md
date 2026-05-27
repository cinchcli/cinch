# Background-running first-close hint — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The first time a user dismisses the desktop window (close box / Cmd+W / Cmd+Q), show a one-time in-window dialog telling them Cinch keeps running in the menu bar — with a real Quit option — instead of silently hiding.

**Architecture:** A persisted flag (`background_hint_seen`) is read in Rust inside the window-dismissal handlers. On the first dismissal the window stays visible and a `BackgroundHint` event is emitted; a self-contained React dialog (`BackgroundHintDialog`, following the existing `SendToast` pattern) renders and reports the user's choice via a `resolve_background_hint(quit)` command, which marks the flag and then hides or quits. Every later dismissal hides silently, exactly as today.

**Tech Stack:** Rust (Tauri v2, tauri-specta), React + TypeScript (Vite, vitest), SQLite settings KV store.

**Spec:** `specs/2026-05-25-background-running-first-close-hint-design.md`

---

## File Structure

| File | Responsibility | Change |
| --- | --- | --- |
| `apps/desktop/src-tauri/src/events.rs` | Specta event definitions | Add `BackgroundHint` unit event |
| `apps/desktop/src-tauri/src/window_manage.rs` | Window placement/dismissal helpers | Add `BACKGROUND_HINT_SEEN_KEY`, pure `should_prompt`, `request_dismiss` + unit test |
| `apps/desktop/src-tauri/src/commands/clips/misc.rs` | Misc window/app commands | Add `resolve_background_hint` command |
| `apps/desktop/src-tauri/src/lib.rs` | App builder, event/command registration, window events | Register event + command; route `CloseRequested` through `request_dismiss` |
| `apps/desktop/src-tauri/src/app_menu.rs` | Custom macOS menu (Cmd+Q) | Route Cmd+Q through `request_dismiss` |
| `apps/desktop/src/bindings.ts` | Generated TS bridge | Regenerated (never hand-edited) |
| `apps/desktop/src/components/BackgroundHintDialog.tsx` | The one-time dialog | **New** — self-contained, event-driven |
| `apps/desktop/src/components/BackgroundHintDialog.test.tsx` | Component test | **New** |
| `apps/desktop/src/App.tsx` | Root UI | Render `<BackgroundHintDialog />` in both branches |

## Hook discipline (read before starting)

`lefthook.yml` enforces:
- **pre-commit:** `cargo fmt --all -- --check` (any `*.rs`), `tsc --noEmit` over `apps/desktop` (any `*.ts(x)`), `version-parity` (always).
- **pre-push:** `cargo test --workspace`.

Consequences baked into the task order below:
1. Run `cargo fmt --all` before every Rust commit.
2. **bindings.ts must be regenerated (Task 5) before committing any `.tsx` that references the new command/event (Tasks 6–7)** — otherwise `tsc --noEmit` fails the commit.
3. Don't touch version manifests.

All commands below assume CWD = repo root `/Users/jinmu/Programming/cinchcli/cinch/claude-background-running-hint` unless stated.

---

## Task 0: Baseline

- [ ] **Step 1: Build the desktop crate (fresh worktree → first build is slow)**

Run: `cd apps/desktop/src-tauri && cargo build -p cinch-desktop`
Expected: compiles (warnings OK), no errors.

- [ ] **Step 2: Frontend deps + test baseline**

Run: `cd apps/desktop && pnpm install && pnpm test`
Expected: install succeeds; existing vitest suite passes.

If either fails, STOP and report — do not start implementing on a red baseline.

---

## Task 1: `BackgroundHint` event

**Files:**
- Modify: `apps/desktop/src-tauri/src/events.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs:107-128` (the `collect_events!` list)

- [ ] **Step 1: Add the event struct**

Append to `apps/desktop/src-tauri/src/events.rs`:

```rust
/// Fired exactly once — the first time the user dismisses the window (close
/// box / Cmd+W / Cmd+Q) before the background-running hint has been
/// acknowledged. The Rust side gates emission on the `background_hint_seen`
/// flag (see `window_manage::request_dismiss`); the React `BackgroundHintDialog`
/// listens and shows the one-time dialog.
#[derive(Clone, Serialize, Deserialize, Type, Event)]
pub struct BackgroundHint;
```

- [ ] **Step 2: Register it in the events list**

In `apps/desktop/src-tauri/src/lib.rs`, inside `collect_events![ … ]` (ends at line 128), add a line after `events::ClipSent,`:

```rust
            events::ClipSent,
            events::BackgroundHint,
```

- [ ] **Step 3: Compile**

Run: `cd apps/desktop/src-tauri && cargo build -p cinch-desktop`
Expected: compiles cleanly.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add apps/desktop/src-tauri/src/events.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): add BackgroundHint specta event"
```

---

## Task 2: Settings key + `should_prompt` + `request_dismiss`

**Files:**
- Modify: `apps/desktop/src-tauri/src/window_manage.rs`

`window_manage.rs` already imports `std::sync::Arc`, `tauri::Manager`, `crate::store`. The pure `should_prompt` is unit-tested; `request_dismiss` is a thin side-effecting wrapper verified by build + manual test.

- [ ] **Step 1: Write the failing test**

Append to `apps/desktop/src-tauri/src/window_manage.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::should_prompt;

    #[test]
    fn test_should_prompt_gates_on_flag() {
        assert!(should_prompt(None), "never seen → prompt");
        assert!(should_prompt(Some("")), "empty → prompt");
        assert!(should_prompt(Some("0")), "not yet acknowledged → prompt");
        assert!(!should_prompt(Some("1")), "acknowledged → do not prompt");
    }
}
```

- [ ] **Step 2: Run the test, verify it fails to compile**

Run: `cd apps/desktop/src-tauri && cargo test -p cinch-desktop should_prompt`
Expected: FAIL — `cannot find function should_prompt in this scope`.

- [ ] **Step 3: Add the constant + `should_prompt` + `request_dismiss`**

Add to `apps/desktop/src-tauri/src/window_manage.rs`, just below the `use` block at the top (after `use crate::PreviousAppPid;`):

```rust
/// Settings-DB key recording that the user has seen the one-time
/// "Cinch keeps running in the menu bar" hint. Value `"1"` once acknowledged.
pub(crate) const BACKGROUND_HINT_SEEN_KEY: &str = "background_hint_seen";

/// Whether the first-dismissal background-running hint should still be shown.
/// True unless the flag has been set to `"1"`.
pub(crate) fn should_prompt(flag: Option<&str>) -> bool {
    flag != Some("1")
}

/// Handle a user-initiated window dismissal (close box / Cmd+W / Cmd+Q).
///
/// On the *first* dismissal (flag unset) it keeps the window visible and emits
/// `BackgroundHint` so the frontend shows the one-time dialog. On every later
/// dismissal — or if the settings DB is unavailable — it hides the `main`
/// window, the existing menu-bar-agent behavior. Programmatic hides
/// (paste-and-hide, overlays) call `window.hide()` directly and must NOT route
/// through here.
pub(crate) fn request_dismiss(app: &tauri::AppHandle) {
    use tauri_specta::Event as _;

    let flag = app
        .try_state::<Arc<store::db::Database>>()
        .and_then(|db| db.get_setting(BACKGROUND_HINT_SEEN_KEY).ok().flatten());

    if should_prompt(flag.as_deref()) {
        // Keep the window visible so the dialog renders over it.
        let _ = crate::events::BackgroundHint.emit(app);
    } else if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}
```

- [ ] **Step 4: Run the test, verify it passes**

Run: `cd apps/desktop/src-tauri && cargo test -p cinch-desktop should_prompt`
Expected: PASS (`test_should_prompt_gates_on_flag ... ok`).

(Note: `request_dismiss` is unused until Task 4 — rustc emits a dead-code *warning*, not an error. That is fine; pre-commit only runs `cargo fmt`, and the warning disappears once Task 4 wires it.)

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add apps/desktop/src-tauri/src/window_manage.rs
git commit -m "feat(desktop): add should_prompt + request_dismiss dismissal gate"
```

---

## Task 3: `resolve_background_hint` command

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands/clips/misc.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs:51-106` (the `collect_commands!` list)

`misc.rs` already imports `State`, `Arc`, and uses `tauri::AppHandle` + `tauri::Manager` (see `focus_previous_app`). The managed DB state is `Arc<crate::store::db::Database>`.

- [ ] **Step 1: Add the command**

Append to `apps/desktop/src-tauri/src/commands/clips/misc.rs`:

```rust
/// Resolve the one-time background-running hint. Marks the hint seen (so it
/// never shows again), then either quits the app (`quit = true`,
/// `app.exit(0)` — the same path as the tray's "Quit Cinch", which passes the
/// `ExitRequested { code: None }` guard in lib.rs) or hides the main window
/// (`quit = false`, the normal menu-bar-agent dismissal).
#[tauri::command]
#[specta::specta]
pub fn resolve_background_hint(
    app: tauri::AppHandle,
    db: State<'_, Arc<crate::store::db::Database>>,
    quit: bool,
) -> Result<(), String> {
    use tauri::Manager;
    db.set_setting(crate::window_manage::BACKGROUND_HINT_SEEN_KEY, "1")?;
    if quit {
        app.exit(0);
    } else if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    Ok(())
}
```

- [ ] **Step 2: Register it in the commands list**

In `apps/desktop/src-tauri/src/lib.rs`, inside `collect_commands![ … ]`, add a line after `commands::clips::set_send_shortcut,` (line 88):

```rust
            commands::clips::set_send_shortcut,
            commands::clips::resolve_background_hint,
```

(`resolve_background_hint` is re-exported through `commands::clips` via `pub use misc::*;` in `commands/clips/mod.rs`.)

- [ ] **Step 3: Compile**

Run: `cd apps/desktop/src-tauri && cargo build -p cinch-desktop`
Expected: compiles cleanly.

- [ ] **Step 4: Commit**

```bash
cargo fmt --all
git add apps/desktop/src-tauri/src/commands/clips/misc.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat(desktop): add resolve_background_hint command"
```

---

## Task 4: Route both dismissal paths through `request_dismiss`

**Files:**
- Modify: `apps/desktop/src-tauri/src/lib.rs:301-304` (`CloseRequested`)
- Modify: `apps/desktop/src-tauri/src/app_menu.rs:20,88-95` (Cmd+Q)

- [ ] **Step 1: Route `CloseRequested` (close box / Cmd+W)**

In `apps/desktop/src-tauri/src/lib.rs`, replace:

```rust
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = window.hide();
            }
```

with:

```rust
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                crate::window_manage::request_dismiss(window.app_handle());
            }
```

(`window` is the `&Window` from `on_window_event`; `window.app_handle()` returns `&AppHandle`.)

- [ ] **Step 2: Route Cmd+Q**

In `apps/desktop/src-tauri/src/app_menu.rs`, replace the body of `handle_menu_event`:

```rust
pub fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    if event.id().as_ref() == HIDE_WINDOW_ID {
        for window in app.webview_windows().values() {
            let _ = window.hide();
        }
    }
}
```

with:

```rust
pub fn handle_menu_event(app: &AppHandle, event: MenuEvent) {
    if event.id().as_ref() == HIDE_WINDOW_ID {
        crate::window_manage::request_dismiss(app);
    }
}
```

- [ ] **Step 3: Drop the now-unused `Manager` import**

Removing `app.webview_windows()` leaves `tauri::Manager` unused in `app_menu.rs`. Change line 20:

```rust
use tauri::{AppHandle, Manager, Wry};
```

to:

```rust
use tauri::{AppHandle, Wry};
```

- [ ] **Step 4: Compile (also confirms `request_dismiss` is no longer dead code)**

Run: `cd apps/desktop/src-tauri && cargo build -p cinch-desktop`
Expected: compiles cleanly — no `unused` warnings for `request_dismiss` or `Manager`.

- [ ] **Step 5: Commit**

```bash
cargo fmt --all
git add apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/app_menu.rs
git commit -m "feat(desktop): show background-running hint on first window dismissal"
```

---

## Task 5: Regenerate TypeScript bindings

**Files:**
- Modify: `apps/desktop/src/bindings.ts` (generated)

- [ ] **Step 1: Regenerate**

Run: `cd apps/desktop/src-tauri && cargo test export_bindings -- --ignored`
Expected: PASS; writes `apps/desktop/src/bindings.ts`.

- [ ] **Step 2: Verify the new symbols landed**

Run: `cd apps/desktop && git diff --stat src/bindings.ts && grep -n "resolveBackgroundHint\|backgroundHint\|BackgroundHint" src/bindings.ts`
Expected: shows a `resolveBackgroundHint(quit: boolean)` command and a `backgroundHint` event entry.

- [ ] **Step 3: Typecheck the generated file in isolation**

Run: `cd apps/desktop && pnpm exec tsc --noEmit`
Expected: PASS (no consumer of the new symbols yet; bindings.ts alone must typecheck).

- [ ] **Step 4: Commit**

```bash
git add apps/desktop/src/bindings.ts
git commit -m "chore(desktop): regenerate bindings for resolve_background_hint"
```

---

## Task 6: `BackgroundHintDialog` component (TDD)

**Files:**
- Create: `apps/desktop/src/components/BackgroundHintDialog.tsx`
- Create: `apps/desktop/src/components/BackgroundHintDialog.test.tsx`

Self-contained and event-driven, mirroring `SendToast`. Visual recipe copied from `ConfirmDialog.tsx`, but with corrected semantics: the safe default ("Keep in menu bar") is the prominent CTA, the initial focus, and what Esc / overlay-click resolve to; "Quit Cinch" is the quiet secondary button.

- [ ] **Step 1: Write the failing test**

Create `apps/desktop/src/components/BackgroundHintDialog.test.tsx`:

```tsx
import { render, screen, act, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";

const h = vi.hoisted(() => ({
  cb: null as null | (() => void),
  resolveBackgroundHint: vi.fn((_quit: boolean) => Promise.resolve(null)),
}));

vi.mock("../bindings", () => ({
  events: {
    backgroundHint: {
      listen: vi.fn((cb: () => void) => {
        h.cb = cb;
        return Promise.resolve(() => {});
      }),
    },
  },
  commands: {
    resolveBackgroundHint: h.resolveBackgroundHint,
  },
}));

import { BackgroundHintDialog } from "./BackgroundHintDialog";

describe("BackgroundHintDialog", () => {
  it("is hidden until the backgroundHint event fires", async () => {
    render(<BackgroundHintDialog />);
    await waitFor(() => expect(h.cb).not.toBeNull());
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("shows on event; 'Keep in menu bar' resolves quit=false and closes", async () => {
    render(<BackgroundHintDialog />);
    await waitFor(() => expect(h.cb).not.toBeNull());
    act(() => h.cb!());
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /keep in menu bar/i }));
    expect(h.resolveBackgroundHint).toHaveBeenCalledWith(false);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("'Quit Cinch' resolves quit=true", async () => {
    render(<BackgroundHintDialog />);
    await waitFor(() => expect(h.cb).not.toBeNull());
    act(() => h.cb!());
    fireEvent.click(screen.getByRole("button", { name: /quit cinch/i }));
    expect(h.resolveBackgroundHint).toHaveBeenCalledWith(true);
  });

  it("Esc resolves quit=false (safe default = keep)", async () => {
    render(<BackgroundHintDialog />);
    await waitFor(() => expect(h.cb).not.toBeNull());
    act(() => h.cb!());
    fireEvent.keyDown(window, { key: "Escape" });
    expect(h.resolveBackgroundHint).toHaveBeenCalledWith(false);
  });
});
```

- [ ] **Step 2: Run the test, verify it fails**

Run: `cd apps/desktop && pnpm exec vitest run src/components/BackgroundHintDialog.test.tsx`
Expected: FAIL — cannot resolve `./BackgroundHintDialog`.

- [ ] **Step 3: Implement the component**

Create `apps/desktop/src/components/BackgroundHintDialog.tsx`:

```tsx
// BackgroundHintDialog — one-time "Cinch keeps running in the menu bar" hint.
//
// Shown the first time the user dismisses the window (close box / Cmd+W /
// Cmd+Q). The Rust side gates this on the `background_hint_seen` flag and emits
// `BackgroundHint` only on the first dismissal; this component renders the
// dialog and reports the user's choice via `resolveBackgroundHint(quit)`.
//
// Self-contained (subscribes to its own event) like SendToast. Reuses the
// ConfirmDialog visual recipe but with its own key/emphasis semantics: the safe
// default ("Keep in menu bar") is the prominent CTA, the initial focus, and what
// Esc / overlay-click resolve to — Quit is a quiet secondary button.

import {
  useEffect,
  useRef,
  useState,
  useCallback,
  useId,
  type CSSProperties,
} from "react";
import { commands, events } from "../bindings";
import { C } from "../design";

// DESIGN.md §6 Level 5 (dark) — same recipe as ConfirmDialog.
const DARK_SHADOW =
  "rgba(0,0,0,0.5) 0 0 0 2px, rgba(255,255,255,0.19) 0 0 14px, rgba(255,255,255,0.05) 0 1px 0 0 inset";
const PRIMARY_GLOW = "rgba(79,179,169,0.18) 0 0 20px 5px";

export function BackgroundHintDialog() {
  const [open, setOpen] = useState(false);
  const keepRef = useRef<HTMLButtonElement | null>(null);
  const titleId = useId();
  const bodyId = useId();

  // Rust emits this only on the first dismissal (flag-gated), so no local
  // "seen" bookkeeping is needed here.
  useEffect(() => {
    const unsub = events.backgroundHint.listen(() => setOpen(true));
    return () => {
      unsub.then((f) => f());
    };
  }, []);

  const resolve = useCallback((quit: boolean) => {
    setOpen(false);
    void commands.resolveBackgroundHint(quit);
  }, []);

  // Esc → keep (safe default).
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        resolve(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, resolve]);

  // Initial focus on the safe default (Keep), after the overlay paints.
  useEffect(() => {
    if (!open) return;
    const raf = requestAnimationFrame(() => keepRef.current?.focus());
    return () => cancelAnimationFrame(raf);
  }, [open]);

  if (!open) return null;

  const styles: Record<string, CSSProperties> = {
    overlay: {
      position: "fixed",
      inset: 0,
      background: "rgba(0,0,0,0.55)",
      zIndex: 200,
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      animation: "confirm-fade-in 200ms cubic-bezier(0.16,1,0.3,1)",
    },
    dialog: {
      background: C.card,
      border: "1px solid var(--border)",
      borderRadius: 12,
      maxWidth: 400,
      width: "calc(100% - 48px)",
      padding: "24px 24px 16px",
      color: C.t1,
      boxShadow: DARK_SHADOW,
      animation:
        "confirm-enter 250ms cubic-bezier(0.16,1,0.3,1), confirm-fade-in 200ms cubic-bezier(0.16,1,0.3,1)",
    },
    title: {
      fontSize: 20,
      fontWeight: 500,
      lineHeight: 1.6,
      letterSpacing: "0.2px",
      color: C.t1,
      marginBottom: 8,
    },
    body: {
      fontSize: 14,
      fontWeight: 500,
      lineHeight: 1.55,
      color: C.t2,
      marginBottom: 20,
    },
    actions: {
      display: "flex",
      justifyContent: "flex-end",
      gap: 8,
      marginTop: 8,
    },
    secondaryBtn: {
      background: "transparent",
      border: `1px solid ${C.borderHover}`,
      color: C.t1,
      fontSize: 12,
      fontWeight: 600,
      letterSpacing: "0.3px",
      padding: "8px 14px",
      borderRadius: 6,
      cursor: "pointer",
    },
    primaryBtn: {
      background: C.t1,
      color: C.bg,
      border: "none",
      fontSize: 12,
      fontWeight: 600,
      letterSpacing: "0.3px",
      padding: "8px 14px",
      borderRadius: 6,
      cursor: "pointer",
      boxShadow: PRIMARY_GLOW,
    },
    hint: {
      fontSize: 12,
      fontWeight: 400,
      color: C.t3,
      marginTop: 12,
      textAlign: "left",
    },
  };

  return (
    <div style={styles.overlay} onClick={() => resolve(false)} role="presentation">
      <div
        className="confirm-dialog"
        style={styles.dialog}
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={bodyId}
      >
        <h2 id={titleId} style={styles.title}>
          Cinch keeps running in the menu bar
        </h2>
        <div id={bodyId} style={styles.body}>
          Closing this window doesn&rsquo;t quit Cinch — it stays in the menu bar
          so your clipboard keeps syncing. Click the menu-bar icon (or press
          &#8984;&#8679;W) to open it again.
        </div>
        <div style={styles.actions}>
          <button
            style={styles.secondaryBtn}
            onClick={() => resolve(true)}
            type="button"
          >
            Quit Cinch
          </button>
          <button
            ref={keepRef}
            style={styles.primaryBtn}
            onClick={() => resolve(false)}
            type="button"
          >
            Keep in menu bar
          </button>
        </div>
        <div style={styles.hint}>Esc keeps Cinch in the menu bar</div>
      </div>
      <style>{`
        @keyframes confirm-fade-in {
          from { opacity: 0; }
          to { opacity: 1; }
        }
        @keyframes confirm-enter {
          from { transform: translateY(8px); }
          to { transform: translateY(0); }
        }
        @media (prefers-reduced-motion: reduce) {
          @keyframes confirm-enter {
            from { transform: none; }
            to { transform: none; }
          }
        }
      `}</style>
    </div>
  );
}
```

- [ ] **Step 4: Run the test, verify it passes**

Run: `cd apps/desktop && pnpm exec vitest run src/components/BackgroundHintDialog.test.tsx`
Expected: PASS (4 tests).

- [ ] **Step 5: Typecheck**

Run: `cd apps/desktop && pnpm exec tsc --noEmit`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src/components/BackgroundHintDialog.tsx apps/desktop/src/components/BackgroundHintDialog.test.tsx
git commit -m "feat(desktop): BackgroundHintDialog one-time menu-bar hint"
```

---

## Task 7: Render the dialog in `App.tsx`

**Files:**
- Modify: `apps/desktop/src/App.tsx` (import + two render sites: ~626 and ~777)

There are two `</main>` branches (setup/unauthenticated and main); the user can dismiss the window in either, so render it in both, right after `<SendToast />`.

(`App.test.tsx` mocks `@tauri-apps/api/event`'s `listen` — not `./bindings` — so the real regenerated `events.backgroundHint.listen` resolves through that mock and the mounted dialog stays inert in those tests. This is why Task 5 must run before this task.)

- [ ] **Step 1: Add the import**

In `apps/desktop/src/App.tsx`, after the existing toast imports (line 29 `import { SendToast } from './components/SendToast';`):

```tsx
import { SendToast } from './components/SendToast';
import { BackgroundHintDialog } from './components/BackgroundHintDialog';
```

- [ ] **Step 2: Render in the setup/unauthenticated branch**

Replace (around line 626):

```tsx
        <ClipDecryptFailedToast />
        <SendToast />
      </main>
```

with:

```tsx
        <ClipDecryptFailedToast />
        <SendToast />
        <BackgroundHintDialog />
      </main>
```

- [ ] **Step 3: Render in the main branch**

Replace (around line 777):

```tsx
      <ClipDecryptFailedToast />
      <SendToast />
      {handoffDialog}
    </main>
```

with:

```tsx
      <ClipDecryptFailedToast />
      <SendToast />
      <BackgroundHintDialog />
      {handoffDialog}
    </main>
```

- [ ] **Step 4: Typecheck + run the App test suite**

Run: `cd apps/desktop && pnpm exec tsc --noEmit && pnpm exec vitest run src/App.test.tsx`
Expected: PASS (App tests still green — the dialog is inert with no event).

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/App.tsx
git commit -m "feat(desktop): mount BackgroundHintDialog in both App branches"
```

---

## Task 8: Full verification

- [ ] **Step 1: Full Rust test suite (pre-push gate)**

Run: `cargo test --workspace`
Expected: PASS, including `test_should_prompt_gates_on_flag`.

- [ ] **Step 2: Full frontend suite + typecheck**

Run: `cd apps/desktop && pnpm exec tsc --noEmit && pnpm test`
Expected: PASS, including the 4 `BackgroundHintDialog` tests.

- [ ] **Step 3: Lint**

Run: `cargo clippy -p cinch-desktop` (from `apps/desktop/src-tauri`)
Expected: no new warnings for the touched files (`request_dismiss`/`Manager` are now used).

- [ ] **Step 4: Manual verification (macOS, `npm run tauri dev`)**

Reset state first: in the dashboard's clip DB, the flag lives in the `settings` table under key `background_hint_seen`. To re-test, delete it (e.g. `sqlite3 ~/Library/Application\ Support/com.cinch.app/clips.db "DELETE FROM settings WHERE key='background_hint_seen';"`), then:

1. Open the window, press the **close box** → dialog appears; window still visible behind it.
2. Click **Keep in menu bar** → window hides; menu-bar icon remains; re-open via Cmd+Shift+W.
3. Close again → hides **silently** (no dialog). ✓ one-time.
4. Reset the flag; close → dialog; press **Esc** → window hides (kept). ✓ safe default.
5. Reset the flag; close → dialog; click **Quit Cinch** → process terminates (menu-bar icon gone). ✓
6. Repeat 1 with **Cmd+Q** instead of the close box → same dialog. ✓
7. Trigger a programmatic hide (e.g. copy a clip to paste-and-hide, if applicable) → **no** dialog. ✓ excluded.

- [ ] **Step 5: Final state check**

Run: `git status` → clean working tree; `git log --oneline` shows the task commits on `agent/claude/background-running-hint`.

---

## Done criteria

- First window dismissal (close box / Cmd+W / Cmd+Q) shows the one-time dialog; later dismissals hide silently.
- "Keep in menu bar" / Esc / overlay-click hide and mark seen; "Quit Cinch" exits the process and marks seen.
- Programmatic hides never show the dialog.
- `cargo test --workspace` and the desktop vitest suite pass; `tsc --noEmit` clean; bindings regenerated, not hand-edited.
