import { useEffect, useState, type CSSProperties } from 'react';
import { C } from '../design';
import { CopyField } from './CopyField';
import { commands } from '../bindings';
import type { AgentResumeConfig, Agent } from '../bindings';
import { unwrap } from '../lib/tauri';
import { IconCheck } from '../icons';

// Verified against README.md + crates/cli. Do not alter these strings without
// re-verifying the CLI surface — settings copy must stay truthful.
const CLAUDE_CMD = 'claude mcp add cinch -- cinch mcp';
const CURSOR_JSON =
  '{ "mcpServers": { "cinch": { "command": "cinch", "args": ["mcp"] } } }';
const CLI_VERSION_CMD = 'cinch --version';
const PULL_CMD = 'cinch pull | pbcopy';

const RESUME_AGENTS: { id: Agent; name: string; cmd: string }[] = [
  { id: 'claude', name: 'Claude Code', cmd: 'claude --resume …' },
  { id: 'codex', name: 'Codex', cmd: 'codex resume …' },
];

const ALL_OFF: AgentResumeConfig = {
  claude_enabled: false,
  codex_enabled: false,
  claude_installed: false,
  codex_installed: false,
  codex_manual_shell: false,
};

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

      {/* ── Resume on exit ──────────────────────────────────── */}
      <ResumeOnExitSection />

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

/** Toggle, per agent, the "copy the resume command when a session ends"
 *  feature. Enabling installs the wiring (Claude SessionEnd hook / Codex shell
 *  wrapper); the backend returns what changed so we can guide the user. */
function ResumeOnExitSection() {
  const [cfg, setCfg] = useState<AgentResumeConfig | null>(null);
  const [busy, setBusy] = useState<Agent | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [snippet, setSnippet] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    (async () => {
      try {
        const c = await unwrap(commands.getAgentResumeConfig());
        if (alive) setCfg(c);
      } catch {
        if (alive) {
          // Don't silently render every toggle as OFF — that would mask a
          // genuinely-enabled config (the opposite of this feature's drift
          // intent). Show the toggles as off but flag that the read failed.
          setCfg(ALL_OFF);
          setError('Couldn’t read agent-resume settings.');
        }
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  const enabledOf = (a: Agent) =>
    a === 'claude' ? cfg?.claude_enabled : cfg?.codex_enabled;
  const installedOf = (a: Agent) =>
    a === 'claude' ? cfg?.claude_installed : cfg?.codex_installed;

  async function toggle(agent: Agent) {
    if (!cfg || busy) return;
    const next = !enabledOf(agent);
    setBusy(agent);
    setError(null);
    setNote(null);
    setSnippet(null);
    try {
      const res = await unwrap(commands.setAgentResumeEnabled(agent, next));
      setCfg((prev) => {
        if (!prev) return prev;
        if (agent === 'claude') {
          return { ...prev, claude_enabled: next, claude_installed: next };
        }
        // Codex is "installed" only when a shell rc was actually auto-edited
        // (a manual-snippet shell leaves files_modified empty).
        const installed = next && res.files_modified.length > 0;
        return { ...prev, codex_enabled: next, codex_installed: installed };
      });
      if (next && res.manual_snippet) {
        setSnippet(res.manual_snippet);
      } else if (res.needs_shell_restart) {
        const where = res.files_modified[0];
        setNote(
          next
            ? where
              ? `Added to ${where} — restart your terminal to apply.`
              : 'Restart your terminal to apply.'
            : 'Removed — restart your terminal to apply.',
        );
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Couldn’t update the setting.');
    } finally {
      setBusy(null);
    }
  }

  return (
    <div style={S.group}>
      <div style={S.heading}>Copy resume command on exit</div>
      <div style={S.desc}>
        When a coding-agent session ends, drop its “resume” command on your
        clipboard and into history — so you can paste it straight back. Saved on
        this Mac only; never synced to your other devices.
      </div>

      {cfg &&
        RESUME_AGENTS.map((a, i) => {
          const enabled = !!enabledOf(a.id);
          // "Drift" means the wiring was auto-installed and later removed by
          // hand. It's meaningless for a manual (fish) shell that was never
          // auto-installable — there, "not installed" is the normal state and
          // the only fix is the pasted snippet, not toggling.
          const manualShell = a.id === 'codex' && !!cfg.codex_manual_shell;
          const drift = enabled && !installedOf(a.id) && !manualShell;
          return (
            <div key={a.id} style={{ marginTop: i === 0 ? 6 : 12 }}>
              <label style={S.toggleRow}>
                <input
                  type="checkbox"
                  checked={enabled}
                  disabled={busy === a.id}
                  onChange={() => void toggle(a.id)}
                  aria-label={`Copy resume command when a ${a.name} session ends`}
                  style={S.srOnlyInput}
                />
                <span
                  aria-hidden="true"
                  style={{ ...S.checkBox, ...(enabled ? S.checkBoxOn : null) }}
                >
                  {enabled && <IconCheck size={11} />}
                </span>
                <span style={S.toggleText}>
                  <span style={S.toggleName}>{a.name}</span>
                  <code style={S.inlineMono}>{a.cmd}</code>
                </span>
              </label>
              {drift && (
                <div style={S.drift}>
                  Enabled, but it isn’t installed — toggle off and on to
                  reinstall.
                </div>
              )}
            </div>
          );
        })}

      {note && <div style={S.note}>{note}</div>}
      {snippet && (
        <div style={{ marginTop: 12 }}>
          <div style={S.hint}>
            Your shell isn’t auto-editable — add this to your shell config:
          </div>
          <CopyField value={snippet} label="Copy Codex shell function" />
        </div>
      )}
      {error && <div style={S.errorText}>{error}</div>}
    </div>
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
  // ─── Resume-on-exit toggle rows ─────────────────────────────
  toggleRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 11,
    cursor: 'pointer',
  },
  srOnlyInput: {
    position: 'absolute',
    width: 1,
    height: 1,
    padding: 0,
    margin: -1,
    overflow: 'hidden',
    clip: 'rect(0 0 0 0)',
    whiteSpace: 'nowrap',
    border: 0,
  },
  checkBox: {
    width: 16,
    height: 16,
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    borderWidth: 1,
    borderStyle: 'solid',
    borderColor: C.borderHover,
    borderRadius: 4,
    color: C.bg,
    transition: 'background 120ms ease, border-color 120ms ease',
  },
  checkBoxOn: {
    background: C.t1,
    borderColor: C.t1,
  },
  toggleText: {
    display: 'flex',
    alignItems: 'baseline',
    gap: 8,
    minWidth: 0,
  },
  toggleName: {
    fontSize: 13,
    fontWeight: 500,
    color: C.t1,
  },
  drift: {
    fontSize: 12,
    fontWeight: 400,
    lineHeight: 1.5,
    color: C.t3,
    marginTop: 4,
    marginLeft: 27,
  },
  errorText: {
    fontSize: 12.5,
    fontWeight: 500,
    color: C.error,
    marginTop: 10,
  },
  inlineMono: {
    fontFamily: 'var(--font-mono)',
    // Match CopyField: keep literal paths/flags ligature-free (no " --" fusing).
    fontFeatureSettings: "'liga' 0, 'calt' 0",
    fontSize: 12,
    color: C.t2,
  },
  divider: {
    border: 'none',
    borderTop: `1px solid ${C.border}`,
    margin: '28px 0',
  },
};
