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
