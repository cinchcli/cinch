import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { AgentsSection } from './AgentsSection';

// The tauri-specta bindings call `invoke` from @tauri-apps/api/core; mock it and
// dispatch by command name (same pattern as SettingsPane.test).
const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));

const ALL_OFF = {
  claude_enabled: false,
  codex_enabled: false,
  claude_installed: false,
  codex_installed: false,
};

beforeEach(() => {
  vi.clearAllMocks();
  Object.assign(navigator, {
    clipboard: { writeText: vi.fn().mockResolvedValue(undefined) },
  });
  invoke.mockImplementation((cmd: string) => {
    if (cmd === 'get_agent_resume_config') return Promise.resolve(ALL_OFF);
    if (cmd === 'set_agent_resume_enabled') {
      return Promise.resolve({
        needs_shell_restart: false,
        files_modified: ['/Users/u/.claude/settings.json'],
        manual_snippet: null,
      });
    }
    return Promise.resolve();
  });
});

describe('AgentsSection', () => {
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

describe('AgentsSection — copy resume command on exit', () => {
  it('enabling Claude calls set_agent_resume_enabled with claude/true', async () => {
    render(<AgentsSection />);
    const cb = await screen.findByLabelText(/Claude Code session ends/i);
    fireEvent.click(cb);
    await waitFor(() => {
      expect(invoke).toHaveBeenCalledWith('set_agent_resume_enabled', {
        agent: 'claude',
        enabled: true,
      });
    });
  });

  it('enabling Codex on a shell with no auto-edit surfaces a manual snippet', async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === 'get_agent_resume_config') return Promise.resolve(ALL_OFF);
      if (cmd === 'set_agent_resume_enabled') {
        return Promise.resolve({
          needs_shell_restart: true,
          files_modified: [],
          manual_snippet: 'function codex\n    command codex $argv\nend',
        });
      }
      return Promise.resolve();
    });
    render(<AgentsSection />);
    const cb = await screen.findByLabelText(/Codex session ends/i);
    fireEvent.click(cb);
    expect(await screen.findByText(/function codex/)).toBeInTheDocument();
  });

  it('flags drift when enabled but not installed', async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === 'get_agent_resume_config') {
        return Promise.resolve({
          ...ALL_OFF,
          claude_enabled: true,
          claude_installed: false,
        });
      }
      return Promise.resolve();
    });
    render(<AgentsSection />);
    expect(
      await screen.findByText(/toggle off and on to reinstall/i),
    ).toBeInTheDocument();
  });

  it('does not flag drift on a manual (fish) shell where Codex was never auto-installable', async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === 'get_agent_resume_config') {
        return Promise.resolve({
          ...ALL_OFF,
          codex_enabled: true,
          codex_installed: false,
          codex_manual_shell: true,
        });
      }
      return Promise.resolve();
    });
    render(<AgentsSection />);
    // The Codex toggle still renders…
    await screen.findByLabelText(/Codex session ends/i);
    // …but the misleading "toggle off and on to reinstall" hint must NOT —
    // toggling can never auto-install on fish, so that advice is dead-end.
    expect(
      screen.queryByText(/toggle off and on to reinstall/i),
    ).not.toBeInTheDocument();
  });

  it('surfaces an error when the initial config load fails', async () => {
    invoke.mockImplementation((cmd: string) => {
      if (cmd === 'get_agent_resume_config') {
        return Promise.reject(new Error('store locked'));
      }
      return Promise.resolve();
    });
    render(<AgentsSection />);
    expect(
      await screen.findByText(/couldn.t read agent-resume settings/i),
    ).toBeInTheDocument();
  });
});
