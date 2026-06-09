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

  it('shows name, email, provider, and user id', async () => {
    render(<AccountIdentity />);
    expect(await screen.findByText('Alice Example')).toBeInTheDocument();
    expect(screen.getByText('alice@example.com')).toBeInTheDocument();
    expect(screen.getByText('github')).toBeInTheDocument();
    expect(screen.getByText('01HZTEST')).toBeInTheDocument();
  });

  it('falls back to em dash when the display name is empty', async () => {
    vi.mocked(commands.getUserProfile).mockResolvedValue({
      display_name: '',
      email: 'alice@example.com',
      identity_provider: 'google',
      user_id: '01HZTEST',
    });
    render(<AccountIdentity />);
    // Email still renders; the Name row shows the em-dash placeholder.
    expect(await screen.findByText('alice@example.com')).toBeInTheDocument();
    expect(screen.getByText('—')).toBeInTheDocument();
  });

  it('renders no editable display-name input', async () => {
    render(<AccountIdentity />);
    await screen.findByText('alice@example.com');
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
  });
});
