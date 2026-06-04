import { type CSSProperties } from 'react';
import { C } from '../design';
import { CopyField } from './CopyField';

// Verified against README.md + crates/cli. Do not alter these strings without
// re-verifying the CLI surface — settings copy must stay truthful.
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
