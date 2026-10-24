// Settings categories share a compact header and scrollable content.
// Retention and clear-history copy follows PRV-02 / PRV-03. Update the UI spec
// before changing those safety disclosures.

import { useEffect, useId, useState, type CSSProperties } from "react";
import { register, unregister, isRegistered } from "@tauri-apps/plugin-global-shortcut";
import { commands, events } from "./bindings";
import type { RetentionConfig, PendingDeviceCode } from "./bindings";
import { unwrap } from "./lib/tauri";
import { physicalKey } from "./lib/keyboard";
import {
  ACTION_META,
  findConflict,
  formatShortcutDisplay,
  DEFAULT_ACTION_SHORTCUTS,
  type ActionId,
  type ActionShortcuts,
} from "./lib/keymap";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { C } from "./design";
import "./SettingsPane.css";
import { IconX, IconLock, IconEye, IconArrowRight, IconCheck } from "./icons";
import ConfirmDialog from "./ConfirmDialog";
import RetentionSlider from "./RetentionSlider";
import { AddRelayDialog } from "./components/AddRelayDialog";
import { DevicesPanel } from "./components/DevicesPanel";
import { useAuthState, signOut } from "./lib/state/auth";
import { useNotifyOnRemoteLogin } from "./lib/settings";
import { PendingLoginCard } from "./components/PendingLoginCard";
import { ManualApproveForm } from "./components/ManualApproveForm";
import { AccountIdentity } from "./components/AccountIdentity";
import { AgentsSection } from "./components/AgentsSection";
import { useTheme, type ThemeMode } from "./lib/state/theme";

const WINDOW_PRESETS = {
  compact:  { label: "Compact",  width: 760,  height: 480 },
  standard: { label: "Standard", width: 960,  height: 600 },
  spacious: { label: "Spacious", width: 1120, height: 720 },
} as const;
type WindowPreset = keyof typeof WINDOW_PRESETS;

function resolveWindowPreset(): WindowPreset {
  const saved = localStorage.getItem("cinch-window-size");
  return (saved && saved in WINDOW_PRESETS ? saved : "standard") as WindowPreset;
}

interface SettingsPaneProps {
  onClose: () => void;
  clipCount: number;
  initialTab?: SettingsTab;
  /** Notifies the parent when a clip-action shortcut changes, so the live
   *  keydown handler in App can update without a restart. */
  onActionShortcutsChange?: (next: ActionShortcuts) => void;
}

type LoadState =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ready"; config: RetentionConfig };

export type SettingsTab = "general" | "privacy" | "devices" | "agents" | "shortcuts";

export const SETTINGS_TABS: SettingsTab[] = ["general", "privacy", "devices", "agents", "shortcuts"];

export const CATEGORY_META: Record<SettingsTab, { label: string; title: string }> = {
  general: { label: "General", title: "General settings" },
  privacy: { label: "Privacy", title: "Storage and privacy" },
  devices: { label: "Devices", title: "Relay and devices" },
  agents: { label: "Agents & CLI", title: "Agents and CLI" },
  shortcuts: { label: "Keyboard", title: "Keyboard shortcuts" },
};

/** Register the global shortcut to show/focus the main window. */
async function registerWindowShortcut(shortcut: string): Promise<void> {
  await register(shortcut, (event) => {
    if (event.state === "Pressed") {
      const win = getCurrentWindow();
      win.show();
      win.setFocus();
    }
  });
}

export default function SettingsPane({ onClose, clipCount, initialTab, onActionShortcutsChange }: SettingsPaneProps) {
  const titleId = useId();
  const auth = useAuthState();
  const { mode, setMode } = useTheme();
  const [activeTab, setActiveTab] = useState<SettingsTab>(initialTab ?? "general");
  const [pending, setPending] = useState<PendingDeviceCode[]>([]);
  const [addRelayOpen, setAddRelayOpen] = useState(false);
  const [disconnectOpen, setDisconnectOpen] = useState(false);
  const [state, setState] = useState<LoadState>({ kind: "loading" });
  const [clearOpen, setClearOpen] = useState(false);
  const [purgeDialog, setPurgeDialog] = useState<{
    nextDays: number;
    count: number;
  } | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);

  const [windowPreset, setWindowPreset] = useState<WindowPreset>(resolveWindowPreset);
  const [notifyOnRemoteLogin, setNotifyOnRemoteLogin] = useNotifyOnRemoteLogin();
  const [autostart, setAutostart] = useState<boolean>(false);

  async function applyWindowPreset(preset: WindowPreset) {
    const { width, height } = WINDOW_PRESETS[preset];
    await getCurrentWindow().setSize(new LogicalSize(width, height));
    localStorage.setItem("cinch-window-size", preset);
    setWindowPreset(preset);
  }

  // Global shortcut state (D-08)
  const [currentShortcut, setCurrentShortcut] = useState<string>("CmdOrCtrl+Shift+W");
  const [shortcutInput, setShortcutInput] = useState<string>("⌘⇧W");
  const [shortcutError, setShortcutError] = useState<string | null>(null);
  const [shortcutSaving, setShortcutSaving] = useState(false);

  // Send shortcut state — opt-in, null means disabled
  const [sendShortcut, setSendShortcut] = useState<string | null>(null);
  const [sendShortcutInput, setSendShortcutInput] = useState<string>("Off");
  const [sendShortcutError, setSendShortcutError] = useState<string | null>(null);
  const [sendShortcutSaving, setSendShortcutSaving] = useState(false);

  // Clip-action shortcuts (Edit / Copy / Pin / Send) — customizable in-app keys.
  const [actionShortcuts, setActionShortcutsState] = useState<ActionShortcuts>(DEFAULT_ACTION_SHORTCUTS);
  const [actionShortcutErrors, setActionShortcutErrors] = useState<Partial<Record<ActionId, string>>>({});
  const [actionCapturing, setActionCapturing] = useState<ActionId | null>(null);

  // Subscribe to device_code_pending events while the pane is mounted.
  useEffect(() => {
    let unsub: (() => void) | null = null;
    let cancelled = false;
    (async () => {
      unsub = await events.deviceCodePending.listen((e) => {
        setPending((prev) =>
          prev.find((p) => p.user_code === e.payload.user_code)
            ? prev
            : [...prev, e.payload],
        );
      });
      if (cancelled) unsub?.();
    })();
    return () => {
      cancelled = true;
      unsub?.();
    };
  }, []);

  // Load persisted global shortcut on mount.
  useEffect(() => {
    unwrap(commands.getGlobalShortcut()).then((s) => {
      setCurrentShortcut(s);
      setShortcutInput(formatShortcutDisplay(s));
    }).catch(() => {/* use defaults */});
  }, []);

  // Load persisted send shortcut on mount.
  useEffect(() => {
    unwrap(commands.getSendShortcut()).then((s) => {
      setSendShortcut(s);
      setSendShortcutInput(s !== null ? formatShortcutDisplay(s) : "Off");
    }).catch(() => {/* default to disabled */});
  }, []);

  // Load persisted clip-action shortcuts on mount.
  useEffect(() => {
    unwrap(commands.getActionShortcuts())
      .then((s) => { if (s) setActionShortcutsState(s); })
      .catch(() => {/* keep defaults */});
  }, []);

  // Load retention config on mount.
  useEffect(() => {
    unwrap(commands.getRetentionConfig())
      .then((config) => setState({ kind: "ready", config }))
      .catch((e: unknown) =>
        setState({ kind: "error", message: String(e) })
      );
  }, []);

  // Load autostart state on mount.
  useEffect(() => {
    unwrap(commands.getAutostart())
      .then((enabled) => setAutostart(enabled))
      .catch(() => {/* use default */});
  }, []);

  async function handleAutostartChange(enabled: boolean) {
    try {
      await unwrap(commands.setAutostart(enabled));
      setAutostart(enabled);
    } catch {
      setAutostart(!enabled); // revert on error
    }
  }

  // Esc closes the pane — but only when no ConfirmDialog is open.
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (clearOpen || purgeDialog) return;
      e.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [clearOpen, purgeDialog, onClose]);

  const currentConfig: RetentionConfig | null =
    state.kind === "ready" ? state.config : null;

  async function commitLocalRetention(next: number) {
    if (!currentConfig) return;
    const current = currentConfig.local_days;
    setSaveError(null);

    if (next < current) {
      try {
        const count = await unwrap(commands.previewRetentionChange(next));
        if (count > 0) {
          setPurgeDialog({ nextDays: next, count });
          return;
        }
      } catch {
        // Preview is advisory — fall through and persist conservatively.
      }
    }

    try {
      await unwrap(commands.setRetentionConfig(next, currentConfig.remote_days));
      setState({
        kind: "ready",
        config: { ...currentConfig, local_days: next },
      });
    } catch (e: unknown) {
      setSaveError(`Couldn't save retention. Try again.`);
      void e;
    }
  }

  async function commitRemoteRetention(next: number) {
    if (!currentConfig) return;
    setSaveError(null);
    try {
      await unwrap(commands.setRetentionConfig(currentConfig.local_days, next));
      setState({
        kind: "ready",
        config: { ...currentConfig, remote_days: next },
      });
    } catch {
      setSaveError(`Couldn't save retention. Try again.`);
    }
  }

  async function confirmPurge() {
    if (!purgeDialog || !currentConfig) return;
    setSaveError(null);
    try {
      await unwrap(commands.setRetentionConfig(purgeDialog.nextDays, currentConfig.remote_days));
      setState({
        kind: "ready",
        config: { ...currentConfig, local_days: purgeDialog.nextDays },
      });
      setPurgeDialog(null);
    } catch {
      setSaveError(`Couldn't save retention. Try again.`);
    }
  }

  async function confirmClear() {
    try {
      await unwrap(commands.clearLocalHistory());
      setClearOpen(false);
      onClose();
    } catch {
      setSaveError(`Couldn't clear history. Try again.`);
    }
  }

  const handleShortcutKeyDown = async (e: React.KeyboardEvent<HTMLInputElement>) => {
    e.preventDefault();
    const modifiers: string[] = [];
    if (e.metaKey || e.ctrlKey) modifiers.push("CmdOrCtrl");
    if (e.shiftKey) modifiers.push("Shift");
    if (e.altKey) modifiers.push("Alt");

    const ignoredKeys = ["Meta", "Control", "Shift", "Alt", "OS"];
    if (ignoredKeys.includes(e.key)) return;

    if (modifiers.length === 0) {
      setShortcutError("Shortcut must include a modifier key");
      return;
    }

    // physicalKey resolves Korean IME (ㅓ on KeyJ) and non-QWERTY layouts to
    // the QWERTY-positioned letter, so the saved accelerator is what Tauri
    // expects (e.g. "CmdOrCtrl+Shift+J", not "CmdOrCtrl+Shift+ㅓ").
    const key = physicalKey(e);
    const newShortcut = [...modifiers, key].join("+");

    setShortcutError(null);
    setShortcutSaving(true);

    try {
      await unwrap(commands.setGlobalShortcut(newShortcut));

      if (await isRegistered(currentShortcut)) {
        await unregister(currentShortcut);
      }
      await registerWindowShortcut(newShortcut);

      setCurrentShortcut(newShortcut);
      setShortcutInput(formatShortcutDisplay(newShortcut));
    } catch {
      setShortcutError("Invalid shortcut");
      try {
        if (!(await isRegistered(currentShortcut))) {
          await registerWindowShortcut(currentShortcut);
        }
      } catch { /* old shortcut may also fail */ }
    } finally {
      setShortcutSaving(false);
    }
  };

  // Handle keydown for the send shortcut capture input.
  // Unlike the window shortcut, we only call commands.setSendShortcut — the
  // Rust backend registers the OS-level shortcut itself. No JS register/unregister.
  const handleSendShortcutKeyDown = async (e: React.KeyboardEvent<HTMLInputElement>) => {
    e.preventDefault();
    const modifiers: string[] = [];
    if (e.metaKey || e.ctrlKey) modifiers.push("CmdOrCtrl");
    if (e.shiftKey) modifiers.push("Shift");
    if (e.altKey) modifiers.push("Alt");

    const ignoredKeys = ["Meta", "Control", "Shift", "Alt", "OS"];
    if (ignoredKeys.includes(e.key)) return;

    if (modifiers.length === 0) {
      setSendShortcutError("Shortcut must include a modifier key");
      return;
    }

    // physicalKey resolves Korean IME / non-QWERTY layouts to the
    // QWERTY-positioned letter (same rationale as the window shortcut above).
    const key = physicalKey(e);
    const newShortcut = [...modifiers, key].join("+");

    setSendShortcutError(null);
    setSendShortcutSaving(true);

    try {
      await unwrap(commands.setSendShortcut(newShortcut));
      setSendShortcut(newShortcut);
      setSendShortcutInput(formatShortcutDisplay(newShortcut));
    } catch {
      setSendShortcutError("Invalid shortcut");
    } finally {
      setSendShortcutSaving(false);
    }
  };

  const handleClearSendShortcut = async () => {
    setSendShortcutError(null);
    setSendShortcutSaving(true);
    try {
      await unwrap(commands.setSendShortcut(null));
      setSendShortcut(null);
      setSendShortcutInput("Off");
    } catch {
      setSendShortcutError("Couldn't clear shortcut");
    } finally {
      setSendShortcutSaving(false);
    }
  };

  // Capture a new clip-action shortcut. Modifier is OPTIONAL here (bare keys
  // are valid for in-app actions); only the conflict check can block a save.
  const handleActionShortcutKeyDown = async (
    action: ActionId,
    e: React.KeyboardEvent<HTMLInputElement>,
  ) => {
    e.preventDefault();
    const ignoredKeys = ["Meta", "Control", "Shift", "Alt", "OS"];
    if (ignoredKeys.includes(e.key)) return;

    const modifiers: string[] = [];
    if (e.metaKey || e.ctrlKey) modifiers.push("CmdOrCtrl");
    if (e.shiftKey) modifiers.push("Shift");
    if (e.altKey) modifiers.push("Alt");

    // physicalKey resolves Korean IME / non-QWERTY layouts to the
    // QWERTY-positioned key, so the saved accelerator matches the keydown
    // handler's own physicalKey comparison.
    const key = physicalKey(e);
    const newShortcut = [...modifiers, key].join("+");

    const conflict = findConflict(action, newShortcut, actionShortcuts);
    if (!conflict.ok) {
      const who =
        conflict.conflictWith === "reserved"
          ? "another shortcut"
          : ACTION_META.find((a) => a.id === conflict.conflictWith)?.label ?? conflict.conflictWith;
      setActionShortcutErrors((p) => ({
        ...p,
        [action]: `${formatShortcutDisplay(newShortcut)} is already used by ${who}`,
      }));
      return;
    }

    const next: ActionShortcuts = { ...actionShortcuts, [action]: newShortcut };
    setActionShortcutErrors((p) => ({ ...p, [action]: undefined }));
    try {
      await unwrap(commands.setActionShortcuts(next));
      setActionShortcutsState(next);
      onActionShortcutsChange?.(next);
      setActionCapturing(null);
    } catch {
      setActionShortcutErrors((p) => ({ ...p, [action]: "Invalid shortcut" }));
    }
  };

  const handleResetActionShortcuts = async () => {
    try {
      const defaults = await unwrap(commands.resetActionShortcuts());
      setActionShortcutsState(defaults);
      setActionShortcutErrors({});
      onActionShortcutsChange?.(defaults);
    } catch {
      /* leave current values in place */
    }
  };

  const meta = CATEGORY_META[activeTab];

  return (
    <div className="settings-pane" role="dialog" aria-modal="true" aria-labelledby={titleId}>
      <header className="settings-header">
        <h1 id={titleId}>Settings</h1>
        <button className="settings-close" onClick={onClose} aria-label="Close settings" type="button">
          <kbd>Esc</kbd><IconX size={14} />
        </button>
      </header>
      <nav className="settings-nav" aria-label="Settings categories">
        {SETTINGS_TABS.map((tab) => (
          <button key={tab} type="button" onClick={() => setActiveTab(tab)}
            aria-current={activeTab === tab ? "page" : undefined}>
            {CATEGORY_META[tab].label}
          </button>
        ))}
      </nav>
      <main className="settings-content" aria-label={meta.title} key={activeTab}>
        <div className="settings-content-inner">
          {/* Devices */}
          {activeTab === "devices" && (
            <>
              <div style={S.fieldGroup}>
                <div style={S.fieldHeading}>Relay server</div>
                {auth.variant === "Authenticated" ? (
                  <div style={S.relayCard}>
                    <div style={S.relayHost}>
                      {(() => { try { return new URL(auth.payload.relay_url).host; } catch { return auth.payload.relay_url; } })()}
                    </div>
                    <div style={S.relayUserId}>{auth.payload.user_id}</div>
                    <div style={{ display: "flex", gap: 8, marginTop: 14 }}>
                      <button
                        type="button"
                        onClick={() => setAddRelayOpen(true)}
                        style={S.ghostBtn}
                      >
                        Re-authenticate
                      </button>
                      <button
                        type="button"
                        onClick={() => setDisconnectOpen(true)}
                        style={{
                          ...S.ghostBtn,
                          color: C.error,
                          borderColor: `color-mix(in srgb, var(--error) 28%, transparent)`,
                        }}
                      >
                        Disconnect
                      </button>
                    </div>
                  </div>
                ) : (
                  <div style={S.relayCard}>
                    <div style={{ ...S.relayUserId, marginBottom: 14 }}>No relay connected.</div>
                    <button
                      type="button"
                      onClick={() => setAddRelayOpen(true)}
                      style={S.primaryBtn}
                    >
                      Connect to relay
                    </button>
                  </div>
                )}
              </div>

              <hr style={S.divider} />

              <div style={S.fieldGroup}>
                <DevicesPanel
                  currentDeviceID={auth.variant === "Authenticated" ? auth.payload.device_id : ""}
                  currentMachineId={auth.variant === "Authenticated" ? auth.payload.machine_id : ""}
                  onShowToast={() => {}}
                />
              </div>

              <hr style={S.divider} />

              <div style={S.fieldGroup}>
                <div style={S.fieldHeading}>Pending sign-ins</div>
                {pending.length > 0 ? (
                  <div style={{ display: "flex", flexDirection: "column", gap: 12, marginBottom: 24 }}>
                    {pending.map((p) => (
                      <PendingLoginCard
                        key={p.user_code}
                        userCode={p.user_code}
                        hostname={p.hostname}
                        sourceRegion={p.source_region}
                        requestedAt={p.requested_at}
                        onResolved={() =>
                          setPending((prev) =>
                            prev.filter((x) => x.user_code !== p.user_code),
                          )
                        }
                      />
                    ))}
                  </div>
                ) : (
                  <div style={S.emptyState}>No pending login requests.</div>
                )}
                <ManualApproveForm onApproved={() => { /* list is already current */ }} />
              </div>

              {addRelayOpen && <AddRelayDialog onClose={() => setAddRelayOpen(false)} />}
              <ConfirmDialog
                open={disconnectOpen}
                title="Disconnect from relay?"
                body="This will sign out and remove your credentials. Your local clip history is kept."
                primaryLabel="Disconnect"
                secondaryLabel="Cancel"
                tone="destructive"
                onConfirm={async () => { setDisconnectOpen(false); await signOut(); onClose(); }}
                onCancel={() => setDisconnectOpen(false)}
              />
            </>
          )}

          {/* Shortcuts */}
          {activeTab === "shortcuts" && (
            <>
              <div style={S.fieldGroup}>
                <div style={S.fieldHeading}>Launch shortcut</div>
                <div style={S.fieldDescription}>
                  Press a new key combination to change the global show-window shortcut.
                </div>
                <input
                  type="text"
                  readOnly
                  value={shortcutSaving ? "Saving…" : shortcutInput}
                  onKeyDown={handleShortcutKeyDown}
                  placeholder="Press a shortcut…"
                  aria-label="Global launch shortcut"
                  style={{
                    ...S.shortcutInput,
                    borderColor: shortcutError ? C.error : C.border,
                  }}
                />
                {shortcutError && (
                  <div style={S.errorRegion}>{shortcutError}</div>
                )}
              </div>

              <hr style={S.divider} />

              <div style={S.fieldGroup}>
                <div style={S.fieldHeading}>Send current clipboard</div>
                <div style={S.fieldDescription}>
                  Press a key combination to send your current clipboard to all connected devices.
                  Off by default — set a shortcut to opt in.
                </div>
                <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                  <input
                    type="text"
                    readOnly
                    value={sendShortcutSaving ? "Saving…" : sendShortcutInput}
                    onKeyDown={handleSendShortcutKeyDown}
                    placeholder="Press a shortcut…"
                    aria-label="Send clipboard shortcut"
                    style={{
                      ...S.shortcutInput,
                      borderColor: sendShortcutError ? C.error : C.border,
                      color: sendShortcut === null ? C.t3 : C.t1,
                    }}
                  />
                  {sendShortcut !== null && (
                    <button
                      type="button"
                      onClick={() => void handleClearSendShortcut()}
                      style={S.ghostBtn}
                      aria-label="Disable send shortcut"
                    >
                      Clear
                    </button>
                  )}
                </div>
                {sendShortcutError && (
                  <div style={S.errorRegion}>{sendShortcutError}</div>
                )}
              </div>

              <hr style={S.divider} />

              <div style={S.fieldGroup}>
                <div style={S.fieldHeading}>Clip actions</div>
                <div style={S.fieldDescription}>
                  Shortcuts for the selected clip. Click a field and press a new
                  combination — bare keys are allowed. ⌘C always copies.
                </div>
                {ACTION_META.map((a, i) => (
                  <div key={a.id}>
                    <div
                      style={{
                        ...S.kbdRow,
                        borderTop: i === 0 ? "none" : `1px solid ${C.border}`,
                      }}
                    >
                      <span style={S.kbdDesc}>{a.label}</span>
                      <input
                        type="text"
                        readOnly
                        value={
                          actionCapturing === a.id
                            ? "Press…"
                            : formatShortcutDisplay(actionShortcuts[a.id])
                        }
                        onFocus={() => setActionCapturing(a.id)}
                        onBlur={() =>
                          setActionCapturing((c) => (c === a.id ? null : c))
                        }
                        onKeyDown={(e) => void handleActionShortcutKeyDown(a.id, e)}
                        aria-label={`${a.label} shortcut`}
                        style={{
                          ...S.shortcutInput,
                          minWidth: 88,
                          textAlign: "center",
                          borderColor: actionShortcutErrors[a.id]
                            ? C.error
                            : C.border,
                        }}
                      />
                    </div>
                    {actionShortcutErrors[a.id] && (
                      <div style={S.errorRegion}>{actionShortcutErrors[a.id]}</div>
                    )}
                  </div>
                ))}
                <button
                  type="button"
                  onClick={() => void handleResetActionShortcuts()}
                  style={{ ...S.ghostBtn, marginTop: 16 }}
                >
                  Reset to defaults
                </button>
              </div>

              <hr style={S.divider} />

              <div style={S.fieldGroup}>
                <div style={S.fieldHeading}>Built-in shortcuts</div>
                <div style={S.fieldDescription}>Custom clip actions take priority over the Pinned and Actions shortcuts.</div>
                {([
                  {
                    group: "Navigation",
                    rows: [
                      { keys: ["↑", "↓"], desc: "Move between clips" },
                      { keys: ["j", "k"], desc: "Move between clips (vim)" },
                      { keys: ["⌘⇧P"], desc: "Show pinned / recent history" },
                      { keys: ["⌘⇧A"], desc: "Open actions" },
                      { keys: ["Tab"], desc: "Move between controls" },
                    ],
                  },
                  {
                    group: "Search",
                    rows: [
                      { keys: ["⌘F", "/"], desc: "Focus search" },
                      { keys: ["Esc"], desc: "Clear search / deselect" },
                    ],
                  },
                  {
                    group: "General",
                    rows: [
                      { keys: ["⌘,"], desc: "Open settings" },
                      { keys: ["Esc"], desc: "Close settings" },
                    ],
                  },
                ] as const).map(({ group, rows }) => (
                  <div key={group} style={S.kbdGroup}>
                    <div style={S.kbdGroupTitle}>{group}</div>
                    <div>
                      {rows.map((row, i) => (
                        <div
                          key={row.desc}
                          style={{
                            ...S.kbdRow,
                            borderTop: i === 0 ? "none" : `1px solid ${C.border}`,
                          }}
                        >
                          <span style={S.kbdDesc}>{row.desc}</span>
                          <div style={{ display: "flex", gap: 4 }}>
                            {row.keys.map((k) => (
                              <kbd key={k} style={S.kbd}>{k}</kbd>
                            ))}
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            </>
          )}

          {/* Agents & CLI */}
          {activeTab === "agents" && <AgentsSection />}

          {/* Privacy */}
          {activeTab === "privacy" && state.kind === "loading" && (
            <div style={S.loading}>Loading retention settings…</div>
          )}

          {activeTab === "privacy" && state.kind === "error" && (
            <div style={S.errorRegion}>
              Couldn't load settings: {state.message}
            </div>
          )}

          {activeTab === "privacy" && state.kind === "ready" && (
            <>
              <div style={S.fieldGroup}>
                <div style={S.fieldHeading}>What the relay can see</div>
                <div style={S.fieldDescription}>
                  Cinch is honest about this — encryption hides your clips, not the
                  fact that they sync.
                </div>
                <dl style={S.trustList}>
                  <div style={{ ...S.trustRow, borderTop: "none", paddingTop: 0 }}>
                    <span style={S.trustIcon} aria-hidden="true">
                      <IconLock size={16} />
                    </span>
                    <div style={S.trustText}>
                      <dt style={S.trustKey}>Clip contents — never</dt>
                      <dd style={S.trustVal}>
                        Text, code, and images are encrypted on this Mac before they
                        leave. The relay only ever stores ciphertext.
                      </dd>
                    </div>
                  </div>
                  <div style={S.trustRow}>
                    <span style={S.trustIcon} aria-hidden="true">
                      <IconEye size={16} />
                    </span>
                    <div style={S.trustText}>
                      <dt style={S.trustKey}>Names, timing &amp; size — yes</dt>
                      <dd style={S.trustVal}>
                        Device names, timestamps, and byte sizes stay visible to the
                        relay — it needs them to route clips to the right machines.
                      </dd>
                    </div>
                  </div>
                  <div style={S.trustRow}>
                    <span style={S.trustIcon} aria-hidden="true">
                      <IconArrowRight size={16} />
                    </span>
                    <div style={S.trustText}>
                      <dt style={S.trustKey}>Want zero metadata?</dt>
                      <dd style={S.trustVal}>
                        Self-host the relay — same app, your server.{" "}
                        <a
                          href="https://cinchcli.com/docs/self-hosting"
                          target="_blank"
                          rel="noopener noreferrer"
                          style={S.trustLink}
                        >
                          Read the self-host guide
                        </a>
                      </dd>
                    </div>
                  </div>
                </dl>
              </div>

              <hr style={S.divider} />

              <div style={S.fieldGroup}>
                <RetentionSlider
                  id="local-retention"
                  label="Local retention"
                  description="How long to keep clips copied on this Mac. Clips older than this are deleted automatically."
                  value={state.config.local_days}
                  onCommit={commitLocalRetention}
                />
              </div>

              <hr style={S.divider} />

              <div style={S.fieldGroup}>
                <RetentionSlider
                  id="remote-retention"
                  label="Remote retention"
                  description="How long to keep clips on the relay server. Clips older than this are deleted from the relay automatically."
                  value={state.config.remote_days}
                  onCommit={commitRemoteRetention}
                />
              </div>

              <hr style={S.divider} />

              <div style={S.fieldGroup}>
                <div style={S.fieldHeading}>Clear history</div>
                <div style={S.fieldDescription}>
                  Delete every clip stored on this Mac. Remote clips on the relay
                  are not affected.
                </div>
                <button
                  type="button"
                  style={{
                    ...S.destructiveBtn,
                    cursor: clipCount === 0 ? "not-allowed" : "pointer",
                    opacity: clipCount === 0 ? 0.4 : 1,
                  }}
                  disabled={clipCount === 0}
                  onClick={() => setClearOpen(true)}
                >
                  {clipCount > 0 ? `Clear ${clipCount} clips…` : "Clear local history"}
                </button>
                <div style={S.disclosure}>
                  Clips from password managers (1Password, Bitwarden, LastPass,
                  Keychain Access) and concealed pasteboard types are never saved.
                </div>
              </div>

              {saveError && <div style={S.errorRegion}>{saveError}</div>}
            </>
          )}

          {activeTab === "general" && (
            <div className="settings-general">
              <section className="settings-row">
                <div>
                  <h2>Account</h2>
                  <p>{auth.variant === "LocalOnly" ? "No account needed for local history." : "Your connected identity."}</p>
                </div>
                {auth.variant === "LocalOnly" ? <span className="settings-value">Local only</span> : <AccountIdentity />}
              </section>
              <section className="settings-row">
                <div>
                  <h2>Theme</h2>
                  <p>Match the system, or choose an appearance.</p>
                </div>
                <div className="settings-segments" role="group" aria-label="Theme">
                  {(["system", "light", "dark"] as ThemeMode[]).map((m) => (
                    <button key={m} type="button" aria-pressed={mode === m} onClick={() => setMode(m)}>
                      {m === "system" ? "System" : m === "light" ? "Light" : "Dark"}
                    </button>
                  ))}
                </div>
              </section>
              <section className="settings-row">
                <div>
                  <h2>Window size</h2>
                  <p>Resize the clipboard window.</p>
                </div>
                <div className="settings-segments" role="group" aria-label="Window size">
                  {(Object.keys(WINDOW_PRESETS) as WindowPreset[]).map((key) => (
                    <button key={key} type="button" aria-pressed={windowPreset === key} onClick={() => void applyWindowPreset(key)}>
                      {WINDOW_PRESETS[key].label}
                    </button>
                  ))}
                </div>
              </section>
              <section className="settings-row">
                <div>
                  <h2>Notifications</h2>
                  <p>Show a notification when a remote login needs approval.</p>
                </div>
                <label className="settings-toggle">
                  <input type="checkbox" checked={notifyOnRemoteLogin}
                    onChange={(e) => setNotifyOnRemoteLogin(e.target.checked)}
                    aria-label="Show macOS notification when a remote login is pending approval"
                    style={S.srOnlyInput} />
                  <span aria-hidden="true" style={{ ...S.checkBox, ...(notifyOnRemoteLogin ? S.checkBoxOn : null) }}>
                    {notifyOnRemoteLogin && <IconCheck size={11} />}
                  </span>
                </label>
              </section>
              <section className="settings-row">
                <div>
                  <h2>Launch at login</h2>
                  <p>Start Cinch when you log in to your Mac.</p>
                </div>
                <label className="settings-toggle">
                  <input type="checkbox" checked={autostart}
                    onChange={(e) => void handleAutostartChange(e.target.checked)}
                    aria-label="Launch cinch at login" style={S.srOnlyInput} />
                  <span aria-hidden="true" style={{ ...S.checkBox, ...(autostart ? S.checkBoxOn : null) }}>
                    {autostart && <IconCheck size={11} />}
                  </span>
                </label>
              </section>
            </div>
          )}

          <ConfirmDialog
            open={clearOpen}
            title={`Delete all ${clipCount} clips?`}
            body={
              <>
                This removes every clip stored on this Mac. Remote clips on the relay
                are not affected. This cannot be undone.
              </>
            }
            primaryLabel="Delete clips"
            secondaryLabel="Cancel"
            tone="destructive"
            onConfirm={confirmClear}
            onCancel={() => setClearOpen(false)}
          />

          <ConfirmDialog
            open={!!purgeDialog}
            title={
              purgeDialog
                ? `Lower local retention to ${purgeDialog.nextDays} days?`
                : ""
            }
            body={
              purgeDialog ? (
                <>
                  {purgeDialog.count} clips older than {purgeDialog.nextDays} days
                  will be deleted from this Mac. Clips newer than{" "}
                  {purgeDialog.nextDays} days are kept.
                </>
              ) : null
            }
            primaryLabel={
              purgeDialog ? `Lower and delete ${purgeDialog.count} clips` : ""
            }
            secondaryLabel="Cancel"
            tone="primary"
            onConfirm={confirmPurge}
            onCancel={() => setPurgeDialog(null)}
          />
        </div>
      </main>
    </div>
  );
}

// ─── Styles ────────────────────────────────────────────────

const S: Record<string, CSSProperties> = {
  // ─── Trust block (what the relay can see) ───────────────
  trustList: {
    margin: 0,
    display: "flex",
    flexDirection: "column",
  },
  trustRow: {
    display: "flex",
    flexDirection: "row",
    gap: 10,
    padding: "10px 0",
    borderTop: `1px solid ${C.border}`,
  },
  trustIcon: {
    flexShrink: 0,
    display: "flex",
    paddingTop: 1,
    color: C.t2,
  },
  trustText: {
    display: "flex",
    flexDirection: "column",
    gap: 3,
    minWidth: 0,
  },
  trustLink: {
    color: C.t2,
    textDecoration: "none",
    borderBottom: `1px solid ${C.borderHover}`,
    cursor: "pointer",
  },
  trustKey: {
    fontSize: 13,
    fontWeight: 500,
    letterSpacing: "-0.005em",
    color: C.t1,
  },
  trustVal: {
    margin: 0,
    fontSize: 12.5,
    fontWeight: 400,
    lineHeight: 1.55,
    color: C.t3,
    maxWidth: 460,
  },
  divider: {
    border: "none",
    borderTop: `1px solid ${C.border}`,
    margin: "20px 0",
  },

  // ─── Field shells ───────────────────────────────────────
  fieldGroup: {
    display: "block",
  },
  fieldHeading: {
    fontSize: 13,
    fontWeight: 500,
    letterSpacing: "-0.005em",
    color: C.t1,
    marginBottom: 4,
  },
  fieldDescription: {
    fontSize: 12,
    fontWeight: 400,
    lineHeight: 1.55,
    color: C.t2,
    marginBottom: 10,
    maxWidth: 440,
  },

  // ─── Controls ───────────────────────────────────────────
  // CSS outlines the adjacent visible control when a hidden checkbox has focus.
  srOnlyInput: {
    position: "absolute",
    width: 1,
    height: 1,
    padding: 0,
    margin: -1,
    overflow: "hidden",
    clip: "rect(0 0 0 0)",
    whiteSpace: "nowrap",
    border: 0,
  },
  checkBox: {
    width: 16,
    height: 16,
    flexShrink: 0,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    // Longhand border props (not the `border` shorthand) so toggling
    // checkBoxOn's borderColor doesn't mix shorthand + longhand on rerender.
    borderWidth: 1,
    borderStyle: "solid",
    borderColor: C.borderHover,
    borderRadius: 4,
    color: C.bg,
    transition: "background 120ms ease, border-color 120ms ease",
  },
  checkBoxOn: {
    background: C.t1,
    borderColor: C.t1,
  },
  shortcutInput: {
    background: C.card,
    border: `1px solid ${C.border}`,
    borderRadius: 6,
    color: C.t1,
    fontSize: 12,
    fontWeight: 500,
    padding: "6px 10px",
    cursor: "pointer",
    letterSpacing: "0.4px",
    width: 160,
    maxWidth: "100%",
    minWidth: 0,
    fontFamily: "var(--font-mono)",
  },

  // ─── Buttons ────────────────────────────────────────────
  ghostBtn: {
    background: "transparent",
    border: `1px solid ${C.border}`,
    borderRadius: 5,
    color: C.t2,
    fontSize: 11.5,
    fontWeight: 500,
    padding: "5px 12px",
    cursor: "pointer",
    fontFamily: "inherit",
    letterSpacing: "0.1px",
  },
  primaryBtn: {
    background: C.t1,
    color: C.bg,
    border: "none",
    borderRadius: 5,
    fontSize: 12,
    fontWeight: 600,
    padding: "7px 16px",
    cursor: "pointer",
    fontFamily: "inherit",
    letterSpacing: "0.1px",
  },
  destructiveBtn: {
    background: "transparent",
    color: C.error,
    border: `1px solid rgba(255, 99, 99, 0.25)`,
    borderRadius: 6,
    padding: "8px 14px",
    fontSize: 12,
    fontWeight: 600,
    letterSpacing: "0.2px",
    fontFamily: "inherit",
  },

  // ─── Relay card ─────────────────────────────────────────
  relayCard: {
    background: "transparent",
    border: "none",
    borderRadius: 0,
    padding: 0,
  },
  relayHost: {
    fontSize: 13,
    fontWeight: 500,
    color: C.t1,
    marginBottom: 4,
    overflowWrap: "anywhere",
    fontFamily: "var(--font-mono)",
    letterSpacing: "-0.005em",
  },
  relayUserId: {
    fontSize: 12,
    overflowWrap: "anywhere",
    color: C.t3,
    fontFamily: "var(--font-mono)",
    letterSpacing: "0.2px",
  },

  // ─── Keyboard reference ─────────────────────────────────
  kbdGroup: {
    marginBottom: 16,
  },
  kbdGroupTitle: {
    fontSize: 11,
    fontWeight: 600,
    letterSpacing: "0.01em",
    color: C.t3,
    marginBottom: 4,
  },
  kbdRow: {
    display: "flex",
    alignItems: "center",
    justifyContent: "space-between",
    padding: "7px 0",
    gap: 12,
  },
  kbdDesc: {
    fontSize: 13,
    fontWeight: 400,
    color: C.t2,
  },
  kbd: {
    background: "var(--kbd-bg)",
    border: `1px solid var(--kbd-border)`,
    borderRadius: 4,
    color: "var(--kbd-color)",
    fontFamily: "var(--font-mono)",
    fontSize: 11.5,
    fontWeight: 500,
    padding: "1px 6px",
    letterSpacing: "0.2px",
    minWidth: 18,
    textAlign: "center",
    display: "inline-block",
  },

  // ─── States ─────────────────────────────────────────────
  disclosure: {
    fontSize: 12,
    fontWeight: 400,
    lineHeight: 1.55,
    color: C.t3,
    marginTop: 12,
    maxWidth: 460,
  },
  errorRegion: {
    fontSize: 13,
    fontWeight: 500,
    color: C.error,
    marginTop: 10,
  },
  loading: {
    fontSize: 13,
    fontWeight: 400,
    color: C.t2,
    padding: "12px 0",
  },
  emptyState: {
    fontSize: 13,
    fontWeight: 400,
    color: C.t3,
    padding: "12px 0",
    marginBottom: 18,
  },
};
