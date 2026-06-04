// SettingsPane — minimalist sidebar+content layout.
//
// Left column: vertical category nav (General / Privacy / Devices / Agents & CLI / Keyboard).
// Right column: scrollable content with generous macro-whitespace and 1px
// dividers between sections — no nested card containers around form fields.
//
// Verbatim copy is load-bearing for PRV-02 / PRV-03 messaging — do NOT
// reword retention/clear-history strings without a UI-SPEC edit.

import { useEffect, useId, useState, type CSSProperties } from "react";
import { register, unregister, isRegistered } from "@tauri-apps/plugin-global-shortcut";
import { commands, events } from "./bindings";
import type { RetentionConfig, PendingDeviceCode } from "./bindings";
import { unwrap } from "./lib/tauri";
import { physicalKey } from "./lib/keyboard";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { LogicalSize } from "@tauri-apps/api/dpi";
import { C } from "./design";
import { IconX, IconCinch } from "./icons";
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
}

type LoadState =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "ready"; config: RetentionConfig };

export type SettingsTab = "general" | "privacy" | "devices" | "agents" | "shortcuts";

export const SETTINGS_TABS: SettingsTab[] = ["general", "privacy", "devices", "agents", "shortcuts"];

export const CATEGORY_META: Record<SettingsTab, { label: string; title: string; subtitle: string }> = {
  general: { label: "General", title: "General", subtitle: "Your account, appearance, window size, and notifications." },
  privacy: { label: "Privacy", title: "Storage & privacy", subtitle: "What the relay can see, how long clips live on this Mac and on the relay, and how to wipe this Mac." },
  devices: { label: "Devices", title: "Relay & devices", subtitle: "Manage your relay connection, the machines linked to this account, and pending sign-ins." },
  agents: { label: "Agents & CLI", title: "Agents & CLI", subtitle: "Connect your coding agent over MCP and use the cinch command line." },
  shortcuts: { label: "Keyboard", title: "Keyboard", subtitle: "Customize the global launch shortcut. The list below shows the built-in shortcuts." },
};

/** Format the internal shortcut string for display using macOS symbols. */
function formatShortcutDisplay(shortcut: string): string {
  return shortcut
    .replace(/CommandOrControl/g, "⌘")
    .replace(/CmdOrCtrl/g, "⌘")
    .replace(/Shift/g, "⇧")
    .replace(/Alt/g, "⌥")
    .replace(/Control/g, "⌃")
    .replace(/\+/g, "");
}

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

export default function SettingsPane({ onClose, clipCount, initialTab }: SettingsPaneProps) {
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
  const [closeHovered, setCloseHovered] = useState(false);
  const [destructiveHovered, setDestructiveHovered] = useState(false);

  const [windowPreset, setWindowPreset] = useState<WindowPreset>(resolveWindowPreset);
  const [notifyOnRemoteLogin, setNotifyOnRemoteLogin] = useNotifyOnRemoteLogin();

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

  // Load retention config on mount.
  useEffect(() => {
    unwrap(commands.getRetentionConfig())
      .then((config) => setState({ kind: "ready", config }))
      .catch((e: unknown) =>
        setState({ kind: "error", message: String(e) })
      );
  }, []);

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

  const meta = CATEGORY_META[activeTab];

  return (
    <div style={S.page} role="dialog" aria-modal="true" aria-labelledby={titleId}>
      {/* Floating close — top-right of the page */}
      <button
        style={{ ...S.closeBtn, background: closeHovered ? C.hover : "transparent" }}
        onClick={onClose}
        onMouseEnter={() => setCloseHovered(true)}
        onMouseLeave={() => setCloseHovered(false)}
        aria-label="Close settings"
        type="button"
      >
        <IconX size={14} />
      </button>

      {/* Left nav */}
      <aside style={S.nav} aria-label="Settings categories">
        <div style={S.navHeader}>
          <span style={S.navLogo} aria-hidden="true">
            <IconCinch size={20} />
          </span>
          <h1 id={titleId} style={S.navTitle}>Settings</h1>
        </div>
        <div style={S.navList}>
          {SETTINGS_TABS.map((tab) => {
            const active = activeTab === tab;
            return (
              <button
                key={tab}
                type="button"
                onClick={() => setActiveTab(tab)}
                aria-current={active ? "page" : undefined}
                style={{
                  ...S.navItem,
                  background: active ? C.selected : "transparent",
                  color: active ? C.t1 : C.t2,
                }}
              >
                {active && <span style={S.navItemBar} aria-hidden="true" />}
                {CATEGORY_META[tab].label}
              </button>
            );
          })}
        </div>
      </aside>

      {/* Right content */}
      <main style={S.content}>
        <div style={S.ambient} aria-hidden="true" />
        <div style={S.contentInner}>
          <header style={S.contentHeader}>
            <h2 style={S.contentTitle}>{meta.title}</h2>
            <p style={S.contentSubtitle}>{meta.subtitle}</p>
          </header>

          <hr style={S.headerDivider} />

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
                <div style={S.fieldHeading}>Remote devices</div>
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
                <div style={S.fieldHeading}>Built-in shortcuts</div>
                {([
                  {
                    group: "Navigation",
                    rows: [
                      { keys: ["↑", "↓"], desc: "Move between clips" },
                      { keys: ["j", "k"], desc: "Move between clips (vim)" },
                      { keys: ["Tab"], desc: "Switch between All / Pinned" },
                    ],
                  },
                  {
                    group: "Actions",
                    rows: [
                      { keys: ["⌘C"], desc: "Copy selected clip" },
                      { keys: ["⌘P"], desc: "Pin / unpin selected clip" },
                      { keys: ["⌘⌫"], desc: "Delete selected clip" },
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
                    <dt style={S.trustKey}>Clip contents — never</dt>
                    <dd style={S.trustVal}>
                      Text, code, and images are encrypted on this Mac before they
                      leave. The relay only ever stores ciphertext.
                    </dd>
                  </div>
                  <div style={S.trustRow}>
                    <dt style={S.trustKey}>Names, timing &amp; size — yes</dt>
                    <dd style={S.trustVal}>
                      Device names, timestamps, and byte sizes stay visible to the
                      relay — it needs them to route clips to the right machines.
                    </dd>
                  </div>
                  <div style={S.trustRow}>
                    <dt style={S.trustKey}>Want zero metadata?</dt>
                    <dd style={S.trustVal}>
                      Self-host the relay — same app, your server.
                    </dd>
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
                  description="How long to keep clips on the relay server. Phase 1 saves this locally; enforcement lands with the next release."
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
                    boxShadow:
                      destructiveHovered && clipCount > 0
                        ? "rgba(255,99,99,0.15) 0 0 20px 5px"
                        : "none",
                  }}
                  disabled={clipCount === 0}
                  onClick={() => setClearOpen(true)}
                  onMouseEnter={() => setDestructiveHovered(true)}
                  onMouseLeave={() => setDestructiveHovered(false)}
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

          {/* General */}
          {activeTab === "general" && (
            <>
              <div style={S.fieldGroup}>
                <div style={S.fieldHeading}>Account</div>
                <div style={S.fieldDescription}>The identity this Mac is signed in as. Read-only.</div>
                <AccountIdentity />
              </div>

              <hr style={S.divider} />

              <div style={S.fieldGroup}>
                <div style={S.fieldHeading}>Theme</div>
                <div style={S.fieldDescription}>Match the system, or pick light or dark.</div>
                <div style={{ display: "flex", gap: 6, marginTop: 4 }}>
                  {(["system", "light", "dark"] as ThemeMode[]).map((m) => {
                    const active = mode === m;
                    return (
                      <button key={m} type="button" onClick={() => setMode(m)}
                        style={{ ...S.segmentBtn, background: active ? C.t1 : "transparent", color: active ? C.bg : C.t2, borderColor: active ? C.t1 : C.border }}>
                        {m === "system" ? "System" : m === "light" ? "Light" : "Dark"}
                      </button>
                    );
                  })}
                </div>
              </div>

              <hr style={S.divider} />

              <div style={S.fieldGroup}>
                <div style={S.fieldHeading}>Window size</div>
                <div style={S.fieldDescription}>Choose a preset size.</div>
                <div style={{ display: "flex", gap: 6, marginTop: 4 }}>
                  {(Object.keys(WINDOW_PRESETS) as WindowPreset[]).map((key) => {
                    const active = windowPreset === key;
                    return (
                      <button key={key} type="button" onClick={() => void applyWindowPreset(key)}
                        style={{ ...S.segmentBtn, background: active ? C.t1 : "transparent", color: active ? C.bg : C.t2, borderColor: active ? C.t1 : C.border }}>
                        {WINDOW_PRESETS[key].label}
                      </button>
                    );
                  })}
                </div>
              </div>

              <hr style={S.divider} />

              <div style={S.fieldGroup}>
                <div style={S.fieldHeading}>Notifications</div>
                <div style={S.fieldDescription}>Control which system notifications cinch shows.</div>
                <label style={S.checkboxRow}>
                  <input type="checkbox" checked={notifyOnRemoteLogin}
                    onChange={(e) => setNotifyOnRemoteLogin(e.target.checked)}
                    aria-label="Show macOS notification when a remote login is pending approval"
                    style={{ accentColor: C.accent }} />
                  <span>Show macOS notification when a remote login is pending approval</span>
                </label>
              </div>
            </>
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
  page: {
    background: C.bg,
    color: C.t1,
    height: "100vh",
    display: "flex",
    flexDirection: "row",
    borderRadius: "var(--radius-xl)",
    border: `1px solid var(--border)`,
    overflow: "hidden",
    position: "relative",
  },
  closeBtn: {
    position: "absolute",
    top: 14,
    right: 16,
    width: 28,
    height: 28,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    border: "none",
    borderRadius: 6,
    color: C.t2,
    cursor: "pointer",
    transition: "background 120ms ease, color 120ms ease",
    zIndex: 2,
  },

  // ─── Left nav ───────────────────────────────────────────
  nav: {
    width: 208,
    flexShrink: 0,
    background: C.card,
    borderRight: `1px solid ${C.border}`,
    display: "flex",
    flexDirection: "column",
    paddingTop: 52,
  },
  navHeader: {
    padding: "0 20px",
    marginBottom: 28,
    display: "flex",
    alignItems: "center",
    gap: 10,
  },
  navLogo: {
    width: 20,
    height: 20,
    display: "inline-flex",
    alignItems: "center",
    justifyContent: "center",
    color: C.t1,
    flexShrink: 0,
  },
  navTitle: {
    fontSize: 18,
    fontWeight: 500,
    letterSpacing: "-0.015em",
    color: C.t1,
    margin: 0,
  },
  navList: {
    display: "flex",
    flexDirection: "column",
    gap: 2,
    padding: "0 8px",
  },
  navItem: {
    position: "relative",
    appearance: "none",
    border: "none",
    fontFamily: "inherit",
    fontSize: 13,
    fontWeight: 500,
    letterSpacing: "0.1px",
    padding: "7px 12px",
    borderRadius: 6,
    textAlign: "left",
    cursor: "pointer",
    transition: "color 120ms ease, background 120ms ease",
  },
  navItemBar: {
    position: "absolute",
    left: -8,
    top: "50%",
    transform: "translateY(-50%)",
    width: 2,
    height: 14,
    background: "var(--accent)",
    borderRadius: 2,
  },

  // ─── Right content ──────────────────────────────────────
  content: {
    flex: 1,
    minWidth: 0,
    overflowY: "auto",
    position: "relative",
  },
  ambient: {
    position: "absolute",
    inset: 0,
    pointerEvents: "none",
    background:
      "radial-gradient(800px 360px at 100% 0%, rgba(237,230,214,0.022), transparent 60%)",
  },
  contentInner: {
    position: "relative",
    maxWidth: 560,
    padding: "52px 56px 88px",
  },
  contentHeader: {
    marginBottom: 0,
  },
  // ─── Trust block (what the relay can see) ───────────────
  trustList: {
    margin: 0,
    display: "flex",
    flexDirection: "column",
  },
  trustRow: {
    display: "flex",
    flexDirection: "column",
    gap: 3,
    padding: "11px 0",
    borderTop: `1px solid ${C.border}`,
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
  contentTitle: {
    fontSize: 26,
    fontWeight: 500,
    letterSpacing: "-0.022em",
    lineHeight: 1.15,
    color: C.t1,
    margin: 0,
  },
  contentSubtitle: {
    fontSize: 13.5,
    fontWeight: 400,
    lineHeight: 1.55,
    color: C.t2,
    marginTop: 8,
    maxWidth: 460,
  },
  headerDivider: {
    border: "none",
    borderTop: `1px solid ${C.border}`,
    margin: "32px 0 28px",
  },
  divider: {
    border: "none",
    borderTop: `1px solid ${C.border}`,
    margin: "28px 0",
  },

  // ─── Field shells ───────────────────────────────────────
  fieldGroup: {
    display: "block",
  },
  fieldHeading: {
    fontSize: 14,
    fontWeight: 600,
    letterSpacing: "-0.005em",
    color: C.t1,
    marginBottom: 4,
  },
  fieldDescription: {
    fontSize: 13,
    fontWeight: 400,
    lineHeight: 1.55,
    color: C.t2,
    marginBottom: 14,
    maxWidth: 440,
  },

  // ─── Controls ───────────────────────────────────────────
  segmentBtn: {
    fontFamily: "inherit",
    fontSize: 12,
    fontWeight: 500,
    letterSpacing: "0.1px",
    padding: "6px 14px",
    borderRadius: 6,
    border: `1px solid ${C.border}`,
    cursor: "pointer",
    transition: "background 120ms ease, color 120ms ease, border-color 120ms ease",
  },
  checkboxRow: {
    display: "flex",
    alignItems: "center",
    gap: 10,
    cursor: "pointer",
    fontSize: 13,
    fontWeight: 400,
    color: C.t2,
    lineHeight: 1.45,
  },
  shortcutInput: {
    background: C.card,
    border: `1px solid ${C.border}`,
    borderRadius: 6,
    color: C.t1,
    fontSize: 14,
    fontWeight: 500,
    padding: "8px 14px",
    outline: "none",
    cursor: "pointer",
    letterSpacing: "0.4px",
    minWidth: 160,
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
    background: C.accent,
    color: C.accentOn,
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
    transition: "box-shadow 180ms ease",
    fontFamily: "inherit",
  },

  // ─── Relay card ─────────────────────────────────────────
  relayCard: {
    background: C.card,
    border: `1px solid ${C.border}`,
    borderRadius: 8,
    padding: "16px 18px",
  },
  relayHost: {
    fontSize: 13,
    fontWeight: 500,
    color: C.t1,
    marginBottom: 4,
    fontFamily: "var(--font-mono)",
    letterSpacing: "-0.005em",
  },
  relayUserId: {
    fontSize: 11.5,
    color: C.t3,
    fontFamily: "var(--font-mono)",
    letterSpacing: "0.2px",
  },

  // ─── Keyboard reference ─────────────────────────────────
  kbdGroup: {
    marginBottom: 20,
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
    padding: "9px 0",
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
    fontSize: 12.5,
    fontWeight: 400,
    lineHeight: 1.55,
    color: C.t3,
    marginTop: 18,
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
