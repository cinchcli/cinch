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
  source_app_id: null,
  source_app: null,
  source_url: null,
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

  it('renders a color preview for standalone color text clips', () => {
    render(
      <ClipDetail
        clip={{ ...baseClip, content: '#fff', byte_size: 4 }}
        onCopy={noOp}
        onPin={noOp}
        onDelete={noOp}
        onSaveImage={noOp}
      />,
    );

    expect(screen.getByLabelText('Color preview for #fff')).toBeInTheDocument();
    expect(screen.getByText('#fff')).toBeInTheDocument();
    expect(screen.getByText('color')).toBeInTheDocument();
  });

  it('does not render a color preview for text that merely contains a color value', () => {
    render(
      <ClipDetail
        clip={{ ...baseClip, content: 'background: #fff;' }}
        onCopy={noOp}
        onPin={noOp}
        onDelete={noOp}
        onSaveImage={noOp}
      />,
    );

    expect(screen.queryByLabelText(/color preview/i)).not.toBeInTheDocument();
    expect(screen.getByText('background: #fff;')).toBeInTheDocument();
  });

  it('shows Copy / Pin / Delete buttons with kbd hints', () => {
    render(<ClipDetail clip={baseClip} onCopy={noOp} onPin={noOp} onDelete={noOp} onSaveImage={noOp} />);
    expect(screen.getByRole('button', { name: /^copy/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^pin/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^delete/i })).toBeInTheDocument();
  });

  it('renders button hints from the configured action shortcuts', () => {
    render(
      <ClipDetail
        clip={baseClip}
        onCopy={noOp}
        onPin={noOp}
        onDelete={noOp}
        onSaveImage={noOp}
        onEdit={noOp}
        actionShortcuts={{
          edit: 'CmdOrCtrl+Shift+E',
          copy: 'CmdOrCtrl+C',
          pin: 'Alt+P',
          send: 'CmdOrCtrl+Enter',
        }}
      />,
    );
    // Hints reflect the passed bindings, not the old hardcoded glyphs.
    expect(screen.getByRole('button', { name: /^edit ⌘⇧E$/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^copy ⌘C$/i })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^pin ⌥P$/i })).toBeInTheDocument();
  });

  it('renders copied app and source URL metadata', () => {
    render(
      <ClipDetail
        clip={{
          ...baseClip,
          source_app_id: 'com.google.Chrome',
          source_app: 'Google Chrome',
          source_url: 'https://example.com/article',
        }}
        onCopy={noOp}
        onPin={noOp}
        onDelete={noOp}
        onSaveImage={noOp}
      />,
    );
    expect(screen.getByText('Google Chrome')).toBeInTheDocument();
    expect(screen.getByText('https://example.com/article')).toBeInTheDocument();
    expect(screen.getByTestId('source-app-icon')).toHaveAttribute(
      'src',
      'cinch://app-icon/com.google.Chrome',
    );
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

  it('shows an Edit button for text clips when onEdit is provided', () => {
    const onEdit = vi.fn();
    render(<ClipDetail clip={baseClip} onCopy={noOp} onPin={noOp} onDelete={noOp} onSaveImage={noOp} onEdit={onEdit} />);
    fireEvent.click(screen.getByRole('button', { name: /^edit/i }));
    expect(onEdit).toHaveBeenCalledWith(baseClip);
  });

  it('hides the Edit button for image clips', () => {
    render(<ClipDetail clip={{ ...baseClip, content_type: 'image' }} onCopy={noOp} onPin={noOp} onDelete={noOp} onSaveImage={noOp} onEdit={vi.fn()} />);
    expect(screen.queryByRole('button', { name: /^edit/i })).not.toBeInTheDocument();
  });
});
