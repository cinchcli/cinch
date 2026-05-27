import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { ClipDetail } from './ClipDetail';
import type { LocalClip } from '../bindings';

const baseClip: LocalClip = {
  id: 'c1',
  user_id: 'u1',
  content: 'hello world',
  content_type: 'text',
  source: 'local',
  label: '',
  byte_size: 11,
  media_path: null,
  created_at: 1_777_614_529,
  synced: false,
  is_pinned: false,
  pin_note: null,
};

const noOp = () => {};

describe('ClipDetail', () => {
  it('renders empty placeholder when no clip selected', () => {
    render(<ClipDetail clip={null} onCopy={noOp} onPin={noOp} onDelete={noOp} onSaveImage={noOp} />);
    expect(screen.getByText(/select a clip/i)).toBeInTheDocument();
  });

  it('renders clip content for selected clip', () => {
    render(<ClipDetail clip={baseClip} onCopy={noOp} onPin={noOp} onDelete={noOp} onSaveImage={noOp} />);
    expect(screen.getByText(/hello world/i)).toBeInTheDocument();
  });

  it('shows Copy / Pin / Delete buttons with kbd hints', () => {
    render(<ClipDetail clip={baseClip} onCopy={noOp} onPin={noOp} onDelete={noOp} onSaveImage={noOp} />);
    expect(screen.getByRole('button', { name: /^copy/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^pin/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^delete/i })).toBeInTheDocument();
  });

  it('calls onCopy when Copy clicked', () => {
    const onCopy = vi.fn();
    render(<ClipDetail clip={baseClip} onCopy={onCopy} onPin={noOp} onDelete={noOp} onSaveImage={noOp} />);
    fireEvent.click(screen.getByRole('button', { name: /^copy/i }));
    expect(onCopy).toHaveBeenCalledWith(baseClip);
  });

  it('renders the image (cinch://media) for an image clip with no media_path', () => {
    // baseClip already has media_path: null — the image branch must still render.
    const imageClip = { ...baseClip, id: 'cimg', content_type: 'image' as const, content: '' };
    const { container } = render(<ClipDetail clip={imageClip} onCopy={noOp} onPin={noOp} onDelete={noOp} onSaveImage={noOp} />);
    const img = container.querySelector('img');
    expect(img).toBeInTheDocument();
    expect(img).toHaveAttribute('src', 'cinch://media/cimg');
  });

  it('shows "Unpin" button when clip is_pinned', () => {
    render(<ClipDetail clip={{ ...baseClip, is_pinned: true }} onCopy={noOp} onPin={noOp} onDelete={noOp} onSaveImage={noOp} />);
    expect(screen.getByRole('button', { name: /^unpin/i })).toBeInTheDocument();
  });

  it('does NOT render a Save button for text clips', () => {
    render(
      <ClipDetail
        clip={baseClip}
        onCopy={noOp}
        onPin={noOp}
        onDelete={noOp}
        onSaveImage={noOp}
      />,
    );
    expect(screen.queryByRole('button', { name: /^save/i })).not.toBeInTheDocument();
  });

  it('renders a Save button for image clips', () => {
    const imageClip = { ...baseClip, id: 'cimg', content_type: 'image' as const, content: '' };
    render(
      <ClipDetail
        clip={imageClip}
        onCopy={noOp}
        onPin={noOp}
        onDelete={noOp}
        onSaveImage={noOp}
      />,
    );
    expect(screen.getByRole('button', { name: /^save/i })).toBeInTheDocument();
  });

  it('calls onSaveImage when Save clicked', () => {
    const imageClip = { ...baseClip, id: 'cimg', content_type: 'image' as const, content: '' };
    const onSaveImage = vi.fn();
    render(
      <ClipDetail
        clip={imageClip}
        onCopy={noOp}
        onPin={noOp}
        onDelete={noOp}
        onSaveImage={onSaveImage}
      />,
      );
    fireEvent.click(screen.getByRole('button', { name: /^save/i }));
    expect(onSaveImage).toHaveBeenCalledWith(imageClip);
  });
});
