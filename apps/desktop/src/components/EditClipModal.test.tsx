import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { EditClipModal } from './EditClipModal';
import type { LocalClip } from '../bindings';

const clip: LocalClip = {
  id: 'c1', user_id: 'u1', content: '![[a.webp|703]]', content_type: 'text',
  source: 'local', source_app_id: null, source_app: null, source_url: null,
  label: '', byte_size: 15, media_path: null, created_at: 1_777_614_529,
  synced: false, is_pinned: false, pin_note: null,
  sync_state: 'local', received_at: 0,
};

describe('EditClipModal', () => {
  it('pre-fills the textarea with the clip content', () => {
    render(<EditClipModal clip={clip} onSave={vi.fn()} onCancel={vi.fn()} />);
    expect(screen.getByRole('textbox')).toHaveValue('![[a.webp|703]]');
  });

  it('calls onSave with the edited text', () => {
    const onSave = vi.fn();
    render(<EditClipModal clip={clip} onSave={onSave} onCancel={vi.fn()} />);
    fireEvent.change(screen.getByRole('textbox'), { target: { value: '![[a.webp]]' } });
    fireEvent.click(screen.getByRole('button', { name: /save/i }));
    expect(onSave).toHaveBeenCalledWith('![[a.webp]]');
  });

  it('calls onCancel on Escape', () => {
    const onCancel = vi.fn();
    render(<EditClipModal clip={clip} onSave={vi.fn()} onCancel={onCancel} />);
    fireEvent.keyDown(window, { key: 'Escape' });
    expect(onCancel).toHaveBeenCalled();
  });

  it('saves on Cmd+Enter', () => {
    const onSave = vi.fn();
    render(<EditClipModal clip={clip} onSave={onSave} onCancel={vi.fn()} />);
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'x' } });
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter', metaKey: true });
    expect(onSave).toHaveBeenCalledWith('x');
  });
});
