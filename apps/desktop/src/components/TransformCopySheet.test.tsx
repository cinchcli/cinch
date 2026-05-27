import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { TransformCopySheet } from './TransformCopySheet';

const actions = [
  { id: 'pretty-json', label: 'Pretty JSON' },
  { id: 'redact-secrets', label: 'Redact Secrets' },
  { id: 'shell-single-quote', label: 'Shell Quote' },
];

describe('TransformCopySheet', () => {
  it('filters actions and confirms the highlighted action', () => {
    const onSelect = vi.fn();
    render(<TransformCopySheet actions={actions} onSelect={onSelect} onClose={vi.fn()} />);

    fireEvent.change(screen.getByRole('textbox', { name: /copy as/i }), {
      target: { value: 'redact' },
    });

    expect(screen.getByText('Redact Secrets')).toBeInTheDocument();
    expect(screen.queryByText('Pretty JSON')).not.toBeInTheDocument();

    fireEvent.keyDown(screen.getByRole('textbox', { name: /copy as/i }), { key: 'Enter' });

    expect(onSelect).toHaveBeenCalledWith('redact-secrets');
  });

  it('closes on Escape', () => {
    const onClose = vi.fn();
    render(<TransformCopySheet actions={actions} onSelect={vi.fn()} onClose={onClose} />);

    fireEvent.keyDown(window, { key: 'Escape' });

    expect(onClose).toHaveBeenCalled();
  });
});
