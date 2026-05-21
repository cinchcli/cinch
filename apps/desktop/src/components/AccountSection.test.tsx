import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { AccountSection } from './AccountSection';
import { commands } from '../bindings';

vi.mock('../bindings', () => ({
  commands: {
    getUserProfile: vi.fn(),
    setDisplayName: vi.fn(),
  },
}));

describe('AccountSection', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(commands.getUserProfile).mockResolvedValue({
      display_name: 'Alice Example',
      email: 'alice@example.com',
      identity_provider: 'github',
      user_id: '01HZTEST',
    });
    // setDisplayName returns typedError<string, string> →
    // { status: "ok"; data: string } | { status: "error"; error: string }
    vi.mocked(commands.setDisplayName).mockResolvedValue({
      status: 'ok',
      data: 'New Name',
    });
  });

  it('shows current display name + email + user_id', async () => {
    render(<AccountSection />);
    expect(await screen.findByDisplayValue('Alice Example')).toBeInTheDocument();
    expect(screen.getByText('alice@example.com')).toBeInTheDocument();
    expect(screen.getByText('01HZTEST')).toBeInTheDocument();
  });

  it('saves a new name via setDisplayName', async () => {
    render(<AccountSection />);
    const input = await screen.findByDisplayValue('Alice Example');
    fireEvent.change(input, { target: { value: 'New Name' } });
    fireEvent.click(screen.getByRole('button', { name: /save/i }));
    await waitFor(() => {
      expect(commands.setDisplayName).toHaveBeenCalledWith('New Name');
    });
  });

  it('rejects empty input without calling setDisplayName', async () => {
    render(<AccountSection />);
    const input = await screen.findByDisplayValue('Alice Example');
    fireEvent.change(input, { target: { value: '   ' } });
    fireEvent.click(screen.getByRole('button', { name: /save/i }));
    await waitFor(() => {
      expect(screen.getByText(/must not be empty/i)).toBeInTheDocument();
    });
    expect(commands.setDisplayName).not.toHaveBeenCalled();
  });

  it('rejects > 64 char input without calling setDisplayName', async () => {
    render(<AccountSection />);
    const input = await screen.findByDisplayValue('Alice Example');
    const long = 'a'.repeat(65);
    fireEvent.change(input, { target: { value: long } });
    fireEvent.click(screen.getByRole('button', { name: /save/i }));
    await waitFor(() => {
      expect(screen.getByText(/64/i)).toBeInTheDocument();
    });
    expect(commands.setDisplayName).not.toHaveBeenCalled();
  });
});
