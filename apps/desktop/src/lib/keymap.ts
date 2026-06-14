// Customizable in-app clip-action shortcuts (Edit / Copy / Pin / Send).
//
// Pure matching/parsing layer built on `physicalKey` so bindings survive
// Korean IME and non-QWERTY layouts, consistent with the rest of the keydown
// handler. No Tauri imports here — this module is unit-testable in isolation.
//
// Accelerator strings use the same vocabulary the Settings capture UI and the
// OS-global shortcuts already use, e.g. "CmdOrCtrl+E", "CmdOrCtrl+Enter",
// "Enter". "CmdOrCtrl" matches either Cmd (macOS) or Ctrl (Windows/Linux).

import { physicalKey } from "./keyboard";

export type ActionId = "edit" | "copy" | "pin" | "send";

/** Structural twin of the specta-generated `ActionShortcuts` in `bindings.ts`. */
export interface ActionShortcuts {
  edit: string;
  copy: string;
  pin: string;
  send: string;
}

export const DEFAULT_ACTION_SHORTCUTS: ActionShortcuts = {
  edit: "CmdOrCtrl+E",
  copy: "Enter",
  pin: "CmdOrCtrl+P",
  send: "CmdOrCtrl+Enter",
};

/** Display order + labels for the Settings "Clip actions" rows. */
export const ACTION_META: { id: ActionId; label: string }[] = [
  { id: "edit", label: "Edit clip" },
  { id: "copy", label: "Copy clip" },
  { id: "pin", label: "Pin / unpin" },
  { id: "send", label: "Send selected" },
];

/**
 * Format an accelerator string ("CmdOrCtrl+Shift+E", "Enter") for display with
 * macOS symbols ("⌘⇧E", "↵"). Shared by Settings and the clip detail buttons so
 * on-screen hints always reflect the configured binding.
 */
export function formatShortcutDisplay(shortcut: string): string {
  return shortcut
    .replace(/CommandOrControl/g, "⌘")
    .replace(/CmdOrCtrl/g, "⌘")
    .replace(/Shift/g, "⇧")
    .replace(/Alt/g, "⌥")
    .replace(/Control/g, "⌃")
    .replace(/\+/g, "")
    .replace(/Enter/g, "↵");
}

const PRIMARY_NAMES = new Set([
  "cmd",
  "command",
  "ctrl",
  "control",
  "meta",
  "super",
  "cmdorctrl",
  "commandorcontrol",
]);
const SHIFT_NAMES = new Set(["shift"]);
const ALT_NAMES = new Set(["alt", "option"]);

export interface ParsedAccel {
  /** Cmd-or-Ctrl required. */
  primary: boolean;
  shift: boolean;
  alt: boolean;
  /** Key token normalized to `physicalKey` form. */
  key: string;
}

// Normalize a key token to the same form `physicalKey` produces: single
// letters uppercased, everything else (Enter, Escape, ArrowUp, "/", ",")
// verbatim. Keeps accelerator-side and event-side comparison apples-to-apples.
function normalizeKeyToken(token: string): string {
  return /^[a-zA-Z]$/.test(token) ? token.toUpperCase() : token;
}

export function parseAccelerator(accel: string): ParsedAccel {
  const parts = accel.split("+").filter((p) => p.length > 0);
  let primary = false;
  let shift = false;
  let alt = false;
  let key = "";
  for (const p of parts) {
    const lower = p.toLowerCase();
    if (PRIMARY_NAMES.has(lower)) primary = true;
    else if (SHIFT_NAMES.has(lower)) shift = true;
    else if (ALT_NAMES.has(lower)) alt = true;
    else key = normalizeKeyToken(p);
  }
  return { primary, shift, alt, key };
}

// Exact-modifier match. A modifier not named in the accelerator must be UP, so
// "CmdOrCtrl+E" does not fire on Cmd+Shift+E, and bare "Enter" does not fire
// while Cmd is held (that's "CmdOrCtrl+Enter").
export function matchesAccelerator(
  e: Pick<
    KeyboardEvent,
    "code" | "key" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey"
  >,
  accel: string,
): boolean {
  const a = parseAccelerator(accel);
  if (!a.key) return false;
  const primaryDown = e.metaKey || e.ctrlKey;
  if (a.primary !== primaryDown) return false;
  if (a.shift !== e.shiftKey) return false;
  if (a.alt !== e.altKey) return false;
  return physicalKey(e) === a.key;
}

// Whether `a` would also trigger a FIXED (non-remappable) handler in the app.
//
// Unlike `matchesAccelerator` (exact modifiers), the fixed handlers in App.tsx
// are deliberately looser, so reserved-collision detection must mirror their
// actual modifier sensitivity — otherwise a Shift/Alt-augmented variant of a
// reserved chord passes this check yet still fires (or double-fires) the fixed
// handler at runtime. `CmdOrCtrl`/`Ctrl` collapse to the same `primary` bucket.
function hitsReservedHandler(a: ParsedAccel): boolean {
  // Single-key handlers that fire on the bare key regardless of ANY modifier
  // (App.tsx: `?` help toggle, Escape, ArrowUp/ArrowDown navigation). So even
  // "CmdOrCtrl+Shift+?" collides with the help toggle.
  if (["?", "Escape", "ArrowUp", "ArrowDown"].includes(a.key)) return true;

  // Bare-key handlers that fire only WITHOUT a primary modifier (`/` focus
  // search, Tab/Shift+Tab panel switch). Their guards ignore Shift/Alt.
  if (!a.primary && (a.key === "/" || a.key === "Tab")) return true;

  // primary+key handlers — `(meta||ctrl) && key` — that ignore Shift/Alt:
  // ⌘C copy alias, ⌘F search, ⌘, settings, ⌘1/2/3 panels, and the Ctrl-based
  // navigation (J/K) and source-filter cycle (H/L).
  if (a.primary && ["C", "F", ",", "1", "2", "3", "J", "K", "H", "L"].includes(a.key)) {
    return true;
  }

  return false;
}

// Canonical key for equality between two remappable actions: primary collapses
// Cmd/Ctrl, modifiers in fixed order, key normalized. Two accelerators that
// fire on the same physical chord (under exact-modifier matching) compare equal.
function canonicalAccel(accel: string): string {
  const a = parseAccelerator(accel);
  const parts: string[] = [];
  if (a.primary) parts.push("primary");
  if (a.shift) parts.push("shift");
  if (a.alt) parts.push("alt");
  parts.push(a.key);
  return parts.join("+");
}

export type ConflictResult =
  | { ok: true }
  | { ok: false; conflictWith: ActionId | "reserved" };

/** True if `accel` collides with a reserved chord or another action's binding. */
export function findConflict(
  actionId: ActionId,
  accel: string,
  all: ActionShortcuts,
): ConflictResult {
  if (hitsReservedHandler(parseAccelerator(accel))) {
    return { ok: false, conflictWith: "reserved" };
  }
  const target = canonicalAccel(accel);
  for (const id of ["edit", "copy", "pin", "send"] as ActionId[]) {
    if (id === actionId) continue;
    if (canonicalAccel(all[id]) === target) {
      return { ok: false, conflictWith: id };
    }
  }
  return { ok: true };
}
