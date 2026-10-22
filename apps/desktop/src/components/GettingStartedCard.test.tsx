import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { GettingStartedCard } from './GettingStartedCard';

describe('GettingStartedCard', () => {
  it('explains local capture without requiring a CLI or account', () => {
    render(<GettingStartedCard onConnectRelay={vi.fn()} />);
    expect(screen.getByText('Your clipboard, on this device.')).toBeInTheDocument();
    expect(screen.getByText(/Copy text or an image in another app/)).toBeInTheDocument();
    expect(screen.getByText(/No account, relay, or internet connection is required/)).toBeInTheDocument();
    expect(screen.queryByText(/cinch push/)).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Copy' })).not.toBeInTheDocument();
  });

  it('opens relay setup only when the optional action is clicked', () => {
    const onConnectRelay = vi.fn();
    render(<GettingStartedCard onConnectRelay={onConnectRelay} />);
    expect(onConnectRelay).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Connect a relay (optional)' }));
    expect(onConnectRelay).toHaveBeenCalledOnce();
  });
});
