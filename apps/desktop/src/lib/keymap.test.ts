import { describe, it, expect } from "vitest";
import {
  parseAccelerator,
  matchesAccelerator,
  findConflict,
  DEFAULT_ACTION_SHORTCUTS,
} from "./keymap";

type EvShape = Pick<
  KeyboardEvent,
  "code" | "key" | "metaKey" | "ctrlKey" | "shiftKey" | "altKey"
>;

function ev(over: Partial<EvShape>): EvShape {
  return {
    code: "",
    key: "",
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    ...over,
  };
}

describe("parseAccelerator", () => {
  it("parses modifiers and an uppercased letter key", () => {
    expect(parseAccelerator("CmdOrCtrl+Shift+E")).toEqual({
      primary: true,
      shift: true,
      alt: false,
      key: "E",
    });
  });

  it("parses a bare named key", () => {
    expect(parseAccelerator("Enter")).toEqual({
      primary: false,
      shift: false,
      alt: false,
      key: "Enter",
    });
  });
});

describe("matchesAccelerator", () => {
  it("matches Cmd+E for the default edit binding", () => {
    expect(
      matchesAccelerator(ev({ code: "KeyE", key: "e", metaKey: true }), "CmdOrCtrl+E"),
    ).toBe(true);
  });

  it("does not match Cmd+Shift+E against Cmd+E (exact modifiers)", () => {
    expect(
      matchesAccelerator(
        ev({ code: "KeyE", key: "e", metaKey: true, shiftKey: true }),
        "CmdOrCtrl+E",
      ),
    ).toBe(false);
  });

  it("matches bare Enter for copy", () => {
    expect(matchesAccelerator(ev({ key: "Enter" }), "Enter")).toBe(true);
  });

  it("bare Enter does not fire while Cmd is held", () => {
    expect(matchesAccelerator(ev({ key: "Enter", metaKey: true }), "Enter")).toBe(false);
  });

  it("Cmd+Enter matches send but not copy", () => {
    expect(
      matchesAccelerator(ev({ key: "Enter", metaKey: true }), "CmdOrCtrl+Enter"),
    ).toBe(true);
    expect(matchesAccelerator(ev({ key: "Enter", metaKey: true }), "Enter")).toBe(false);
  });

  it("CmdOrCtrl matches either Cmd or Ctrl", () => {
    expect(
      matchesAccelerator(ev({ code: "KeyP", key: "p", metaKey: true }), "CmdOrCtrl+P"),
    ).toBe(true);
    expect(
      matchesAccelerator(ev({ code: "KeyP", key: "p", ctrlKey: true }), "CmdOrCtrl+P"),
    ).toBe(true);
  });

  it("survives Korean IME via physical key code", () => {
    // KeyE position types 'ㄷ' under the Korean 2-bul layout; physicalKey
    // resolves it to "E" from e.code.
    expect(
      matchesAccelerator(ev({ code: "KeyE", key: "ㄷ", metaKey: true }), "CmdOrCtrl+E"),
    ).toBe(true);
  });
});

describe("findConflict", () => {
  it("passes a clean unique binding", () => {
    expect(findConflict("edit", "CmdOrCtrl+E", DEFAULT_ACTION_SHORTCUTS)).toEqual({
      ok: true,
    });
  });

  it("flags a duplicate within the four actions", () => {
    // copy's default is bare Enter; rebinding edit to Enter collides with it.
    const r = findConflict("edit", "Enter", DEFAULT_ACTION_SHORTCUTS);
    expect(r).toEqual({ ok: false, conflictWith: "copy" });
  });

  it("flags a reserved collision (Cmd+C copy alias)", () => {
    const r = findConflict("edit", "CmdOrCtrl+C", DEFAULT_ACTION_SHORTCUTS);
    expect(r).toEqual({ ok: false, conflictWith: "reserved" });
  });

  it("treats Cmd and Ctrl as the same primary chord", () => {
    // Ctrl+J is reserved (nav down); binding to Cmd+J must also be blocked.
    const r = findConflict("edit", "CmdOrCtrl+J", DEFAULT_ACTION_SHORTCUTS);
    expect(r).toEqual({ ok: false, conflictWith: "reserved" });
  });

  it("reserves Shift+Tab (shadowed by the fixed reverse-panel handler)", () => {
    const r = findConflict("edit", "Shift+Tab", DEFAULT_ACTION_SHORTCUTS);
    expect(r).toEqual({ ok: false, conflictWith: "reserved" });
  });

  it("reserves Shift+? (the chord the help toggle actually produces)", () => {
    const r = findConflict("edit", "Shift+?", DEFAULT_ACTION_SHORTCUTS);
    expect(r).toEqual({ ok: false, conflictWith: "reserved" });
  });

  it("reserves Shift-augmented variants of fixed primary chords", () => {
    // The fixed ⌘F/⌘C/⌘,/⌘1-3 and Ctrl+J/K/H/L handlers check (meta||ctrl)+key
    // and ignore Shift, so a Shift+ variant still fires them at runtime and
    // must be blocked even though matchesAccelerator() is exact-modifier.
    for (const accel of ["CmdOrCtrl+Shift+F", "CmdOrCtrl+Shift+C", "Ctrl+Shift+J"]) {
      expect(findConflict("edit", accel, DEFAULT_ACTION_SHORTCUTS)).toEqual({
        ok: false,
        conflictWith: "reserved",
      });
    }
  });

  it("reserves CmdOrCtrl+Shift+? (help toggle fires on '?' under any modifiers)", () => {
    // The help handler matches `key === '?'` regardless of Cmd/Shift, so the
    // primary+shift variant the capture UI can emit also collides.
    expect(findConflict("edit", "CmdOrCtrl+Shift+?", DEFAULT_ACTION_SHORTCUTS)).toEqual({
      ok: false,
      conflictWith: "reserved",
    });
  });

  it("allows a Shift chord that hits no fixed handler", () => {
    // ⌘⇧E is free: no fixed handler owns E, and it differs from the default
    // edit binding (⌘E) by the Shift modifier under exact matching.
    expect(findConflict("send", "CmdOrCtrl+Shift+E", DEFAULT_ACTION_SHORTCUTS)).toEqual({
      ok: true,
    });
  });
});
