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
