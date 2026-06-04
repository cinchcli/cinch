# Desktop Settings Re-plan — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Re-plan the desktop Settings into five intent-named sections (General · Privacy · Devices · Agents & CLI · Shortcuts), add a new informational "Agents & CLI" section with verified MCP/CLI commands, and remove the unused Display name field.

**Architecture:** Frontend-only (React/TS in `apps/desktop/src`). Three new small components (`CopyField`, `AccountIdentity`, `AgentsSection`), a rewired `SettingsPane.tsx` (tab metadata + section routing + `initialTab`), and `App.tsx` wiring so the onboarding "what can the server see?" deep-link lands on the new Privacy tab. One optional read-only Rust command (`get_cli_status`) at the end.

**Tech Stack:** Tauri v2 + React 19 + TypeScript. Tests: vitest + @testing-library/react (jsdom). Inline-style design system via `C` tokens from `src/design`. Clipboard via `navigator.clipboard.writeText` (existing pattern in `CleanupDialog.tsx`). Icons from `src/icons` (`IconCopy` available).

**Reference spec:** `specs/2026-06-04-desktop-settings-replan-design.md`.

**Honesty guardrails (load-bearing — see spec §5/§10):**
- `cinch push` is **local-only** and there is **no `cinch send`** → never show `cmd | cinch push` as a cross-machine send.
- `cinch pull` reads the relay → `cinch pull | pbcopy` is the truthful cross-machine example.
- `cinch mcp` is read-only over the **local** store; never imply remote/"fleet" reads.

**Commands (run from `apps/desktop/`):**
- Tests: `pnpm test` (alias for `vitest run`); single file: `pnpm exec vitest run src/components/CopyField.test.tsx`
- Typecheck/build: `pnpm build` (runs `tsc` then `vite build`); fast typecheck: `pnpm exec tsc --noEmit`

---

## File Structure

- **Create** `apps/desktop/src/components/CopyField.tsx` — reusable mono command box + copy button.
- **Create** `apps/desktop/src/components/CopyField.test.tsx`
- **Create** `apps/desktop/src/components/AccountIdentity.tsx` — read-only email/provider/user ID `<dl>`.
- **Create** `apps/desktop/src/components/AccountIdentity.test.tsx`
- **Create** `apps/desktop/src/components/AgentsSection.tsx` — MCP connect + CLI quickstart.
- **Create** `apps/desktop/src/components/AgentsSection.test.tsx`
- **Modify** `apps/desktop/src/SettingsPane.tsx` — tab type/metadata, nav casing, section routing, `initialTab`, theme control.
- **Delete** `apps/desktop/src/components/AccountSection.tsx` + `AccountSection.test.tsx` (replaced by `AccountIdentity`).
- **Modify** `apps/desktop/src/App.tsx` — `openSettings(tab)` helper, onboarding → `privacy`, pass `initialTab`.
- **(Optional) Create** `apps/desktop/src-tauri/src/commands/cli_status.rs` + register + regen bindings + wire into `AgentsSection`.

---

## Task 1: `CopyField` component

**Files:**
- Create: `apps/desktop/src/components/CopyField.tsx`
- Test: `apps/desktop/src/components/CopyField.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
// apps/desktop/src/components/CopyField.test.tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { CopyField } from './CopyField';

describe('CopyField', () => {
  beforeEach(() => {
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
    });
  });

  it('renders the exact value', () => {
    render(<CopyField value="claude mcp add cinch -- cinch mcp" />);
    expect(
      screen.getByText('claude mcp add cinch -- cinch mcp'),
    ).toBeInTheDocument();
  });

  it('copies the exact value to the clipboard on click', async () => {
    render(<CopyField value="cinch --version" label="Copy version command" />);
    fireEvent.click(screen.getByRole('button', { name: /copy version command/i }));
    await waitFor(() => {
      expect(navigator.clipboard.writeText).toHaveBeenCalledWith('cinch --version');
    });
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/components/CopyField.test.tsx`
Expected: FAIL — `Failed to resolve import './CopyField'` (module not found).

- [ ] **Step 3: Write minimal implementation**

```tsx
// apps/desktop/src/components/CopyField.tsx
import { useState, type CSSProperties } from 'react';
import { C } from '../design';
import { IconCopy } from '../icons';

interface CopyFieldProps {
  /** The exact text shown in the box and written to the clipboard. */
  value: string;
  /** Accessible label for the copy button. */
  label?: string;
}

export function CopyField({ value, label }: CopyFieldProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    navigator.clipboard.writeText(value).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  return (
    <div style={S.row}>
      <code style={S.code}>{value}</code>
      <button
        type="button"
        onClick={handleCopy}
        style={S.btn}
        aria-label={label ?? 'Copy command'}
      >
        {copied ? 'Copied' : <IconCopy size={14} />}
      </button>
    </div>
  );
}

const S: Record<string, CSSProperties> = {
  row: {
    display: 'flex',
    alignItems: 'stretch',
    gap: 8,
    marginTop: 6,
  },
  code: {
    flex: 1,
    minWidth: 0,
    background: C.card,
    border: `1px solid ${C.border}`,
    borderRadius: 6,
    color: C.t1,
    fontFamily: 'var(--font-mono)',
    fontSize: 12.5,
    lineHeight: 1.5,
    padding: '8px 12px',
    overflowX: 'auto',
    whiteSpace: 'pre',
  },
  btn: {
    flexShrink: 0,
    minWidth: 64,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    background: 'transparent',
    border: `1px solid ${C.border}`,
    borderRadius: 6,
    color: C.t2,
    fontSize: 11.5,
    fontWeight: 500,
    fontFamily: 'inherit',
    letterSpacing: '0.1px',
    cursor: 'pointer',
    padding: '0 10px',
  },
};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/components/CopyField.test.tsx`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/components/CopyField.tsx apps/desktop/src/components/CopyField.test.tsx
git commit -m "feat(desktop): add CopyField reusable copy-command component"
```

---

## Task 2: `AgentsSection` component (MCP connect + CLI quickstart)

**Files:**
- Create: `apps/desktop/src/components/AgentsSection.tsx`
- Test: `apps/desktop/src/components/AgentsSection.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
// apps/desktop/src/components/AgentsSection.test.tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { AgentsSection } from './AgentsSection';

describe('AgentsSection', () => {
  beforeEach(() => {
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
    });
  });

  it('shows the verified Claude Code MCP command', () => {
    render(<AgentsSection />);
    expect(
      screen.getByText('claude mcp add cinch -- cinch mcp'),
    ).toBeInTheDocument();
  });

  it('shows the verified Cursor mcp.json snippet', () => {
    render(<AgentsSection />);
    expect(screen.getByText(/"mcpServers"/)).toBeInTheDocument();
  });

  it('shows the truthful cinch pull example and never cinch push', () => {
    render(<AgentsSection />);
    expect(screen.getByText('cinch pull | pbcopy')).toBeInTheDocument();
    expect(screen.queryByText(/cinch push/)).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/components/AgentsSection.test.tsx`
Expected: FAIL — `Failed to resolve import './AgentsSection'`.

- [ ] **Step 3: Write minimal implementation**

```tsx
// apps/desktop/src/components/AgentsSection.tsx
import { type CSSProperties } from 'react';
import { C } from '../design';
import { CopyField } from './CopyField';

// Verified against README.md + crates/cli. Do not alter these strings without
// re-verifying the CLI surface — settings copy must stay truthful (spec §5/§10).
const CLAUDE_CMD = 'claude mcp add cinch -- cinch mcp';
const CURSOR_JSON =
  '{ "mcpServers": { "cinch": { "command": "cinch", "args": ["mcp"] } } }';
const CLI_VERSION_CMD = 'cinch --version';
const PULL_CMD = 'cinch pull | pbcopy';

export function AgentsSection() {
  return (
    <section aria-label="Agents and CLI settings">
      {/* ── MCP ─────────────────────────────────────────────── */}
      <div style={S.group}>
        <div style={S.heading}>Connect your agent (MCP)</div>
        <div style={S.desc}>
          Let your coding agent search and read this Mac&apos;s clipboard
          history — including clips synced here from your other devices —
          read-only. Cinch never sends a clip to an AI provider on its own.
        </div>

        <div style={S.label}>Claude Code</div>
        <CopyField value={CLAUDE_CMD} label="Copy Claude Code MCP command" />

        <div style={{ ...S.label, marginTop: 16 }}>Cursor</div>
        <div style={S.hint}>
          Add to <code style={S.inlineMono}>~/.cursor/mcp.json</code> (or a
          project <code style={S.inlineMono}>.cursor/mcp.json</code>):
        </div>
        <CopyField value={CURSOR_JSON} label="Copy Cursor MCP config" />

        <div style={S.note}>
          Read-only tools: <code style={S.inlineMono}>search_clipboard</code>,{' '}
          <code style={S.inlineMono}>list_recent_clipboard</code>,{' '}
          <code style={S.inlineMono}>get_clipboard_item</code>.
        </div>
      </div>

      <hr style={S.divider} />

      {/* ── CLI ─────────────────────────────────────────────── */}
      <div style={S.group}>
        <div style={S.heading}>Command line</div>
        <div style={S.desc}>
          The <code style={S.inlineMono}>cinch</code> CLI ships with this app.
          Confirm it&apos;s on your PATH:
        </div>
        <CopyField value={CLI_VERSION_CMD} label="Copy version command" />

        <div style={{ ...S.label, marginTop: 16 }}>Pull on any machine</div>
        <div style={S.hint}>
          Print the most recent clip from any of your devices to stdout:
        </div>
        <CopyField value={PULL_CMD} label="Copy pull command" />
      </div>
    </section>
  );
}

const S: Record<string, CSSProperties> = {
  group: { display: 'block' },
  heading: {
    fontSize: 14,
    fontWeight: 600,
    letterSpacing: '-0.005em',
    color: C.t1,
    marginBottom: 4,
  },
  desc: {
    fontSize: 13,
    fontWeight: 400,
    lineHeight: 1.55,
    color: C.t2,
    marginBottom: 14,
    maxWidth: 460,
  },
  label: {
    fontSize: 12,
    fontWeight: 600,
    color: C.t2,
    letterSpacing: '0.01em',
  },
  hint: {
    fontSize: 12.5,
    fontWeight: 400,
    lineHeight: 1.5,
    color: C.t3,
    marginTop: 4,
  },
  note: {
    fontSize: 12.5,
    fontWeight: 400,
    lineHeight: 1.55,
    color: C.t3,
    marginTop: 14,
    maxWidth: 460,
  },
  inlineMono: {
    fontFamily: 'var(--font-mono)',
    fontSize: 12,
    color: C.t2,
  },
  divider: {
    border: 'none',
    borderTop: `1px solid ${C.border}`,
    margin: '28px 0',
  },
};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/components/AgentsSection.test.tsx`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/components/AgentsSection.tsx apps/desktop/src/components/AgentsSection.test.tsx
git commit -m "feat(desktop): add Agents & CLI settings section (MCP + CLI, verified copy)"
```

---

## Task 3: `AccountIdentity` component (read-only identity)

**Files:**
- Create: `apps/desktop/src/components/AccountIdentity.tsx`
- Test: `apps/desktop/src/components/AccountIdentity.test.tsx`

- [ ] **Step 1: Write the failing test**

```tsx
// apps/desktop/src/components/AccountIdentity.test.tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { AccountIdentity } from './AccountIdentity';
import { commands } from '../bindings';

vi.mock('../bindings', () => ({
  commands: { getUserProfile: vi.fn() },
}));

describe('AccountIdentity', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.getUserProfile).mockResolvedValue({
      display_name: 'Alice Example',
      email: 'alice@example.com',
      identity_provider: 'github',
      user_id: '01HZTEST',
    });
  });

  it('shows email, provider, and user id', async () => {
    render(<AccountIdentity />);
    expect(await screen.findByText('alice@example.com')).toBeInTheDocument();
    expect(screen.getByText('github')).toBeInTheDocument();
    expect(screen.getByText('01HZTEST')).toBeInTheDocument();
  });

  it('renders no editable display-name input', async () => {
    render(<AccountIdentity />);
    await screen.findByText('alice@example.com');
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/components/AccountIdentity.test.tsx`
Expected: FAIL — `Failed to resolve import './AccountIdentity'`.

- [ ] **Step 3: Write minimal implementation**

```tsx
// apps/desktop/src/components/AccountIdentity.tsx
import { useEffect, useState, type CSSProperties } from 'react';
import { commands } from '../bindings';
import { C } from '../design';

/** Read-only identity rows for the signed-in account. No display name. */
export function AccountIdentity() {
  const [email, setEmail] = useState('');
  const [provider, setProvider] = useState('');
  const [userId, setUserId] = useState('');

  useEffect(() => {
    let mounted = true;
    (async () => {
      const p = await commands.getUserProfile();
      if (!mounted) return;
      setEmail(p.email);
      setProvider(p.identity_provider);
      setUserId(p.user_id);
    })();
    return () => {
      mounted = false;
    };
  }, []);

  return (
    <dl style={S.dl}>
      <div style={S.dlRow}>
        <dt style={S.dt}>Email</dt>
        <dd style={S.dd}>{email || '—'}</dd>
      </div>
      <div style={S.dlRow}>
        <dt style={S.dt}>Provider</dt>
        <dd style={S.dd}>{provider || '—'}</dd>
      </div>
      <div style={S.dlRow}>
        <dt style={S.dt}>User ID</dt>
        <dd style={S.dd}>
          <code style={S.mono}>{userId || '—'}</code>
        </dd>
      </div>
    </dl>
  );
}

const S: Record<string, CSSProperties> = {
  dl: { display: 'flex', flexDirection: 'column', gap: 0, margin: 0, padding: 0 },
  dlRow: {
    display: 'flex',
    alignItems: 'baseline',
    gap: 12,
    padding: '9px 0',
    borderBottom: `1px solid ${C.border}`,
  },
  dt: {
    fontSize: 12,
    fontWeight: 600,
    color: C.t3,
    letterSpacing: '0.01em',
    minWidth: 72,
    flexShrink: 0,
  },
  dd: {
    fontSize: 13,
    fontWeight: 400,
    color: C.t1,
    margin: 0,
    fontFamily: 'var(--font-body)',
    wordBreak: 'break-all',
  },
  mono: {
    fontFamily: 'var(--font-mono)',
    fontSize: 12,
    color: C.t2,
    letterSpacing: '0.2px',
  },
};
```

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm exec vitest run src/components/AccountIdentity.test.tsx`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/components/AccountIdentity.tsx apps/desktop/src/components/AccountIdentity.test.tsx
git commit -m "feat(desktop): add read-only AccountIdentity (replaces display-name editor)"
```

---

## Task 4: Rewire `SettingsPane.tsx` to the five-section IA

**Files:**
- Modify: `apps/desktop/src/SettingsPane.tsx`
- Test: `apps/desktop/src/SettingsPane.tabs.test.tsx` (new, pure-data test)
- Delete: `apps/desktop/src/components/AccountSection.tsx`, `apps/desktop/src/components/AccountSection.test.tsx`

This task changes metadata + section routing. The trust block, retention sliders, clear-history block, relay card, devices panel, and sessions block are **relocated verbatim** between tab conditionals — only their `activeTab === "…"` guards change. The General section is rebuilt.

- [ ] **Step 1: Write the failing test (pure tab metadata)**

```tsx
// apps/desktop/src/SettingsPane.tabs.test.tsx
import { describe, it, expect } from 'vitest';
import { SETTINGS_TABS, CATEGORY_META } from './SettingsPane';

describe('Settings IA', () => {
  it('exposes exactly the five new tabs in order', () => {
    expect(SETTINGS_TABS).toEqual([
      'general',
      'privacy',
      'devices',
      'agents',
      'shortcuts',
    ]);
  });

  it('labels the new section "Agents & CLI" with correct casing', () => {
    expect(CATEGORY_META.agents.label).toBe('Agents & CLI');
  });

  it('no longer has account or sessions/servers tabs', () => {
    expect(SETTINGS_TABS).not.toContain('account');
    expect(SETTINGS_TABS).not.toContain('servers');
    expect(SETTINGS_TABS).not.toContain('sessions');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm exec vitest run src/SettingsPane.tabs.test.tsx`
Expected: FAIL — `SETTINGS_TABS`/`CATEGORY_META` not exported (or wrong values).

- [ ] **Step 3a: Replace imports**

In `apps/desktop/src/SettingsPane.tsx`, remove the AccountSection import and add the new ones. Replace:

```tsx
import { AccountSection } from "./components/AccountSection";
```

with:

```tsx
import { AccountIdentity } from "./components/AccountIdentity";
import { AgentsSection } from "./components/AgentsSection";
import { useTheme, type ThemeMode } from "./lib/state/theme";
```

- [ ] **Step 3b: Replace the `Tab` type + `CATEGORY_META` with the exported five-section version**

Replace this block:

```tsx
type Tab = "general" | "account" | "shortcuts" | "servers" | "sessions";

const CATEGORY_META: Record<Tab, { label: string; title: string; subtitle: string }> = {
  general: { /* …old… */ },
  account: { /* …old… */ },
  shortcuts: { /* …old… */ },
  servers: { /* …old… */ },
  sessions: { /* …old… */ },
};
```

with:

```tsx
export type SettingsTab = "general" | "privacy" | "devices" | "agents" | "shortcuts";

export const SETTINGS_TABS: SettingsTab[] = [
  "general",
  "privacy",
  "devices",
  "agents",
  "shortcuts",
];

export const CATEGORY_META: Record<
  SettingsTab,
  { label: string; title: string; subtitle: string }
> = {
  general: {
    label: "General",
    title: "General",
    subtitle: "Your account, appearance, window size, and notifications.",
  },
  privacy: {
    label: "Privacy",
    title: "Storage & privacy",
    subtitle:
      "What the relay can see, how long clips live on this Mac and on the relay, and how to wipe this Mac.",
  },
  devices: {
    label: "Devices",
    title: "Relay & devices",
    subtitle:
      "Manage your relay connection, the machines linked to this account, and pending sign-ins.",
  },
  agents: {
    label: "Agents & CLI",
    title: "Agents & CLI",
    subtitle: "Connect your coding agent over MCP and use the cinch command line.",
  },
  shortcuts: {
    label: "Keyboard",
    title: "Keyboard",
    subtitle:
      "Customize the global launch shortcut. The list below shows the built-in shortcuts.",
  },
};
```

- [ ] **Step 3c: Add the `initialTab` prop and seed `activeTab`**

Replace:

```tsx
interface SettingsPaneProps {
  onClose: () => void;
  clipCount: number;
}
```

with:

```tsx
interface SettingsPaneProps {
  onClose: () => void;
  clipCount: number;
  /** Which section to open on first render. Defaults to "general". */
  initialTab?: SettingsTab;
}
```

In the component signature, accept it and seed state. Replace:

```tsx
export default function SettingsPane({ onClose, clipCount }: SettingsPaneProps) {
  const titleId = useId();
  const auth = useAuthState();
  const [activeTab, setActiveTab] = useState<Tab>("general");
```

with:

```tsx
export default function SettingsPane({ onClose, clipCount, initialTab }: SettingsPaneProps) {
  const titleId = useId();
  const auth = useAuthState();
  const { mode, setMode } = useTheme();
  const [activeTab, setActiveTab] = useState<SettingsTab>(initialTab ?? "general");
```

- [ ] **Step 3d: Fix the nav rendering (casing + tab list)**

Replace the nav `.map`:

```tsx
{(Object.keys(CATEGORY_META) as Tab[]).map((tab) => {
```

with:

```tsx
{SETTINGS_TABS.map((tab) => {
```

And in the `S.navItem` style object, **remove** the line `textTransform: "capitalize",` (labels are now already cased, so "Agents & CLI" must not become "Agents & Cli").

- [ ] **Step 3e: Rebuild the section bodies in `<main>`**

Find the content blocks rendered by `activeTab`. Apply these changes:

1. **Delete** the account line entirely:
   ```tsx
   {/* Account */}
   {activeTab === "account" && <AccountSection />}
   ```

2. **Sessions → Devices guard.** Change `{activeTab === "sessions" && (` to be part of devices. Concretely: cut the entire existing `{activeTab === "sessions" && ( … )}` JSX block (the pending list + `ManualApproveForm`) and paste it at the end of the servers block (inside it), then rename the servers guard. See step 3 below for the merged result; remove the standalone sessions block.

3. **Servers → Devices.** Change the guard `{activeTab === "servers" && (` to `{activeTab === "devices" && (`, and append a "Pending sign-ins" sub-block (the relocated sessions content) before the closing `</>`. The merged Devices block reads:

   ```tsx
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
               <button type="button" onClick={() => setAddRelayOpen(true)} style={S.ghostBtn}>
                 Re-authenticate
               </button>
               <button
                 type="button"
                 onClick={() => setDisconnectOpen(true)}
                 style={{ ...S.ghostBtn, color: C.error, borderColor: `color-mix(in srgb, var(--error) 28%, transparent)` }}
               >
                 Disconnect
               </button>
             </div>
           </div>
         ) : (
           <div style={S.relayCard}>
             <div style={{ ...S.relayUserId, marginBottom: 14 }}>No relay connected.</div>
             <button type="button" onClick={() => setAddRelayOpen(true)} style={S.primaryBtn}>
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
           <div style={{ display: "flex", flexDirection: "column", gap: 12, marginBottom: 18 }}>
             {pending.map((p) => (
               <PendingLoginCard
                 key={p.user_code}
                 userCode={p.user_code}
                 hostname={p.hostname}
                 sourceRegion={p.source_region}
                 requestedAt={p.requested_at}
                 onResolved={() =>
                   setPending((prev) => prev.filter((x) => x.user_code !== p.user_code))
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
   ```

4. **Agents** — add after the devices block:

   ```tsx
   {/* Agents & CLI */}
   {activeTab === "agents" && <AgentsSection />}
   ```

5. **General → Privacy split.** The three existing `{activeTab === "general" && state.kind === "…"}` blocks own retention/trust/clear and must move to `privacy`. Change each guard `activeTab === "general"` to `activeTab === "privacy"` for the loading, error, and ready blocks. Then **remove** the "Window size" and "Notifications" field groups from the (now privacy) ready block — they move to General in the next step. The privacy ready block keeps exactly: the trust block (`What the relay can see`), local `RetentionSlider`, remote `RetentionSlider`, the clear-history field group, and the trailing `{saveError && …}`.

6. **New General block** — add (it does not depend on the retention `state`):

   ```tsx
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
               <button
                 key={m}
                 type="button"
                 onClick={() => setMode(m)}
                 style={{
                   ...S.segmentBtn,
                   background: active ? C.t1 : "transparent",
                   color: active ? C.bg : C.t2,
                   borderColor: active ? C.t1 : C.border,
                 }}
               >
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
               <button
                 key={key}
                 type="button"
                 onClick={() => void applyWindowPreset(key)}
                 style={{
                   ...S.segmentBtn,
                   background: active ? C.t1 : "transparent",
                   color: active ? C.bg : C.t2,
                   borderColor: active ? C.t1 : C.border,
                 }}
               >
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
           <input
             type="checkbox"
             checked={notifyOnRemoteLogin}
             onChange={(e) => setNotifyOnRemoteLogin(e.target.checked)}
             aria-label="Show macOS notification when a remote login is pending approval"
             style={{ accentColor: C.accent }}
           />
           <span>Show macOS notification when a remote login is pending approval</span>
         </label>
       </div>
     </>
   )}
   ```

- [ ] **Step 3f: Delete the obsolete AccountSection files**

```bash
git rm apps/desktop/src/components/AccountSection.tsx apps/desktop/src/components/AccountSection.test.tsx
```

- [ ] **Step 4: Run the tab test + typecheck + full suite**

Run: `pnpm exec vitest run src/SettingsPane.tabs.test.tsx`
Expected: PASS (3 tests).

Run: `pnpm exec tsc --noEmit`
Expected: no errors. (If `useId`, `WINDOW_PRESETS`, etc. report unused, ensure all referenced symbols still exist; the `Tab` type name must be fully replaced by `SettingsTab` everywhere in the file.)

Run: `pnpm test`
Expected: PASS — no test references the deleted `AccountSection`.

- [ ] **Step 5: Commit**

```bash
git add apps/desktop/src/SettingsPane.tsx apps/desktop/src/SettingsPane.tabs.test.tsx
git commit -m "feat(desktop): re-plan settings into 5 sections (General/Privacy/Devices/Agents & CLI/Shortcuts)"
```

---

## Task 5: Wire `App.tsx` (open-on-tab + onboarding → Privacy)

**Files:**
- Modify: `apps/desktop/src/App.tsx`

The onboarding "What can the server see? →" button must open Settings on the new **Privacy** tab (that's where the trust block now lives).

- [ ] **Step 1: Import the tab type**

Add to the existing `import SettingsPane from './SettingsPane';` line area:

```tsx
import SettingsPane, { type SettingsTab } from './SettingsPane';
```

(Replace the existing default-only import.)

- [ ] **Step 2: Add tab state + an `openSettings` helper**

Find `const [showSettings, setShowSettings] = useState(false);` and add directly after it:

```tsx
  const [settingsTab, setSettingsTab] = useState<SettingsTab>('general');
  const openSettings = (tab: SettingsTab = 'general') => {
    setSettingsTab(tab);
    setShowSettings(true);
  };
```

- [ ] **Step 3: Route the open call sites**

- Tray listener — replace `events.trayOpenSettings.listen(() => setShowSettings(true))` with
  `events.trayOpenSettings.listen(() => openSettings())`.
- `Cmd+,` toggle — replace `setShowSettings(v => !v);` with
  `setSettingsTab('general'); setShowSettings(v => !v);`.
- Rail — replace `onOpenSettings={() => setShowSettings(true)}` with
  `onOpenSettings={() => openSettings()}`.
- Onboarding — replace `onShowSettings={() => setShowSettings(true)}` with
  `onShowSettings={() => openSettings('privacy')}`.

- [ ] **Step 4: Pass `initialTab` to `SettingsPane`**

Replace:

```tsx
        <SettingsPane
          onClose={() => { setShowSettings(false); if (auth.variant === 'Authenticated') refreshDevices(); }}
          clipCount={totalClips}
        />
```

with:

```tsx
        <SettingsPane
          onClose={() => { setShowSettings(false); if (auth.variant === 'Authenticated') refreshDevices(); }}
          clipCount={totalClips}
          initialTab={settingsTab}
        />
```

- [ ] **Step 5: Typecheck + suite + manual verify**

Run: `pnpm exec tsc --noEmit`
Expected: no errors.

Run: `pnpm test`
Expected: PASS (all suites).

Manual (run `pnpm tauri dev` from `apps/desktop/`, or `make dev-desktop` from repo root):
1. Open Settings (`Cmd+,`) → lands on **General**; identity rows show email/provider/user ID; **no Display name input**; theme segmented control switches light/dark live.
2. Nav shows: General · Privacy · Devices · Agents & CLI · Shortcuts (note literal "Agents & CLI" casing).
3. **Privacy** shows the trust block + both retention sliders + Clear history.
4. **Devices** shows relay card + remote devices + Pending sign-ins.
5. **Agents & CLI** shows the two MCP commands + `cinch --version` + `cinch pull | pbcopy`; copy buttons flash "Copied".
6. From the signed-out **OnboardingScreen**, click "What can the server see? →" → Settings opens directly on **Privacy**.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src/App.tsx
git commit -m "feat(desktop): open Settings on a target tab; onboarding trust link → Privacy"
```

---

## Task 6 (OPTIONAL): live `cinch` CLI status row

Only do this if you want the §5.2 status row to show *installed · path · version* live instead of static guidance. **Caveat:** a macOS GUI app launched from Finder has a minimal `PATH` that often excludes `/opt/homebrew/bin`, so `which cinch` may report "not found" even when the Homebrew cask installed it. The static guidance (Task 2) is the reliable floor; treat this as best-effort enrichment, not a correctness signal.

**Files:**
- Create: `apps/desktop/src-tauri/src/commands/cli_status.rs`
- Modify: `apps/desktop/src-tauri/src/commands/mod.rs` (declare module), `apps/desktop/src-tauri/src/lib.rs` (register command + specta)
- Regenerate: `apps/desktop/src/bindings.ts`
- Modify: `apps/desktop/src/components/AgentsSection.tsx` + its test

- [ ] **Step 1: Write the Rust command (+ unit test)**

```rust
// apps/desktop/src-tauri/src/commands/cli_status.rs
use serde::Serialize;
use specta::Type;
use std::process::Command;

/// Best-effort detection of the `cinch` CLI on the user's PATH.
#[derive(Debug, Clone, Serialize, Type)]
pub struct CliStatus {
    pub on_path: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn get_cli_status() -> CliStatus {
    let path = resolve_cinch();
    let version = path.as_ref().and_then(|_| cinch_version());
    CliStatus { on_path: path.is_some(), path, version }
}

fn resolve_cinch() -> Option<String> {
    let out = Command::new("/usr/bin/which").arg("cinch").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!p.is_empty()).then_some(p)
}

fn cinch_version() -> Option<String> {
    let out = Command::new("cinch").arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let v = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!v.is_empty()).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::CliStatus;

    #[test]
    fn cli_status_serializes_not_found_shape() {
        let s = CliStatus { on_path: false, path: None, version: None };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["on_path"], false);
        assert!(v["path"].is_null());
        assert!(v["version"].is_null());
    }
}
```

- [ ] **Step 2: Declare the module + register the command**

In `apps/desktop/src-tauri/src/commands/mod.rs`, add `pub mod cli_status;` alongside the other module declarations.

In `apps/desktop/src-tauri/src/lib.rs`, add `commands::cli_status::get_cli_status,` to both the `tauri::generate_handler!`/`collect_commands!` list and the specta `Builder::commands(...)` collection (match the existing pattern used by `commands::auth::get_user_profile` — add it in the same two places that symbol appears).

- [ ] **Step 3: Build Rust + regenerate bindings**

Run: `cd apps/desktop/src-tauri && cargo test export_bindings -- --ignored`
Expected: writes `apps/desktop/src/bindings.ts`; `getCliStatus` and `CliStatus` now appear in it.

Run: `cargo test -p cinch-desktop cli_status`
Expected: `cli_status_serializes_not_found_shape` PASS.

- [ ] **Step 4: Wire into `AgentsSection` (replace the static version row)**

Add at the top of `AgentsSection`:

```tsx
import { useEffect, useState } from 'react';
import { commands } from '../bindings';
// …
  const [cli, setCli] = useState<{ on_path: boolean; path: string | null; version: string | null } | null>(null);
  useEffect(() => {
    let mounted = true;
    commands.getCliStatus().then((s) => { if (mounted) setCli(s); }).catch(() => {});
    return () => { mounted = false; };
  }, []);
```

Render a status line above the `CLI_VERSION_CMD` `CopyField`:

```tsx
{cli?.on_path ? (
  <div style={S.hint}>
    Installed{cli.version ? ` · ${cli.version}` : ''}{cli.path ? ` · ${cli.path}` : ''}
  </div>
) : (
  <div style={S.hint}>Not detected on this app&apos;s PATH — confirm with:</div>
)}
```

Update `AgentsSection.test.tsx`: add `vi.mock('../bindings', () => ({ commands: { getCliStatus: vi.fn().mockResolvedValue({ on_path: false, path: null, version: null }) } }));` and keep the three existing assertions.

- [ ] **Step 5: Typecheck + tests**

Run: `pnpm exec tsc --noEmit && pnpm exec vitest run src/components/AgentsSection.test.tsx`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add apps/desktop/src-tauri/src/commands/cli_status.rs apps/desktop/src-tauri/src/commands/mod.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src/bindings.ts apps/desktop/src/components/AgentsSection.tsx apps/desktop/src/components/AgentsSection.test.tsx
git commit -m "feat(desktop): live cinch CLI status in Agents & CLI section"
```

---

## Task 7: Final verification

- [ ] **Step 1: Full desktop test suite**

Run: `cd apps/desktop && pnpm test`
Expected: all suites PASS, including `CopyField`, `AccountIdentity`, `AgentsSection`, `SettingsPane.tabs`. No reference to `AccountSection` remains.

- [ ] **Step 2: Typecheck / build**

Run: `pnpm build`
Expected: `tsc` clean, `vite build` succeeds.

- [ ] **Step 3: Grep guards (honesty + dead refs)**

Run: `grep -rn "cinch push" apps/desktop/src/components/AgentsSection.tsx` → Expected: no output.
Run: `grep -rn "AccountSection" apps/desktop/src` → Expected: no output.
Run: `grep -rn "setDisplayName" apps/desktop/src` → Expected: no output in components (the binding may remain in `bindings.ts`; that's fine and noted as the backend follow-up).

- [ ] **Step 4: Lint (optional, repo-level)**

Run (repo root): `make lint`
Expected: passes (fmt/clippy unaffected if Task 6 skipped).

---

## Self-review notes (author)

- **Spec coverage:** §4 IA → Task 4; §4.2 theme → Task 4 step 3e/3f general block; §5 Agents & CLI → Task 2 (+ optional Task 6); §6 display-name removal → Task 3 + Task 4 deletion; §7 optional backend → Task 6; §8 deep-link → Task 5; §11 tests → Tasks 1–4 + Task 7.
- **No placeholders:** every code step shows full code; relocations name the exact guard lines to change.
- **Type consistency:** `SettingsTab` (exported from SettingsPane, imported by App), `SETTINGS_TABS`, `CATEGORY_META`, `CliStatus`/`get_cli_status`↔`getCliStatus` are used identically across tasks.
- **Out of scope (unchanged from spec §3):** display-name wire/relay removal, device fingerprint/verification, danger-zone restyle, top-level approval badge, Settings-as-drawer.
