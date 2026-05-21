import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('@tauri-apps/plugin-deep-link', () => ({
  onOpenUrl: vi.fn(() => Promise.resolve(() => {})),
  getCurrent: vi.fn(() => Promise.resolve(null)),
}));

import { UpgradePrompt } from './UpgradePrompt';

describe('UpgradePrompt', () => {
  it('renders "Sign in for cross-machine sync" copy', () => {
    render(<UpgradePrompt onDismiss={vi.fn()} />);
    // Text is split: <button>Sign in</button> + text node "for cross-machine sync".
    // Check the interactive button and adjacent text separately.
    expect(screen.getByRole('button', { name: 'Sign in' })).toBeInTheDocument();
    expect(screen.getByText('for cross-machine sync')).toBeInTheDocument();
  });

  it('renders dismiss button with aria-label "Dismiss upgrade prompt"', () => {
    render(<UpgradePrompt onDismiss={vi.fn()} />);
    expect(screen.getByLabelText('Dismiss upgrade prompt')).toBeInTheDocument();
  });

  it('calls onDismiss callback when dismiss button clicked', () => {
    const onDismiss = vi.fn();
    render(<UpgradePrompt onDismiss={onDismiss} />);
    fireEvent.click(screen.getByLabelText('Dismiss upgrade prompt'));
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it('uses C.card background', () => {
    const { container } = render(<UpgradePrompt onDismiss={vi.fn()} />);
    const wrapper = container.firstElementChild as HTMLElement;
    const style = wrapper.getAttribute('style') || '';
    expect(style).toContain('var(--surface)');
  });

  it('has border-top using C.border', () => {
    const { container } = render(<UpgradePrompt onDismiss={vi.fn()} />);
    const wrapper = container.firstElementChild as HTMLElement;
    const style = wrapper.getAttribute('style') || '';
    expect(style).toContain('var(--border)');
  });
});
