import { describe, it, expect, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';
import { ClipDetail } from './ClipDetail';
import type { LocalClip } from '../bindings';

vi.mock('../bindings', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../bindings')>();
  return {
    ...actual,
    commands: {
      ...actual.commands,
      getAppIcon: vi.fn(async (bundleId: string) => (
        bundleId.startsWith('com.apple.') ? `data:image/png;base64,${bundleId}` : null
      )),
    },
  };
});

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

describe('ClipDetail', () => {
  it('renders a placeholder when no clip is selected', () => {
    render(<ClipDetail clip={null} />);
    expect(screen.getByText(/select a clip/i)).toBeInTheDocument();
  });

  it('renders raw text without a repeated title or action toolbar', () => {
    render(<ClipDetail clip={baseClip} />);
    expect(screen.getByText(baseClip.content)).toBeInTheDocument();
    expect(screen.queryByRole('heading')).not.toBeInTheDocument();
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  it('preserves code whitespace and highlights search matches', () => {
    const content = 'function run() {\n  return true;\n}';
    const { container } = render(<ClipDetail clip={{ ...baseClip, content, content_type: 'code' }} searchQuery="return" />);
    expect(container.querySelector('pre')?.textContent).toBe(content);
    expect(container.querySelector('mark')).toHaveTextContent('return');
  });

  it('renders standalone colors but not color fragments in prose', () => {
    const { rerender } = render(<ClipDetail clip={{ ...baseClip, content: '#fff' }} />);
    expect(screen.getByLabelText('Color preview for #fff')).toBeInTheDocument();
    expect(screen.getByText('color')).toBeInTheDocument();
    rerender(<ClipDetail clip={{ ...baseClip, content: 'background: #fff;' }} />);
    expect(screen.queryByLabelText(/Color preview/)).not.toBeInTheDocument();
    expect(screen.getByText('background: #fff;')).toBeInTheDocument();
  });

  it('retains source, copied app and source URL metadata', async () => {
    render(<ClipDetail clip={{ ...baseClip, source: 'remote:host', source_app: 'Safari', source_app_id: 'com.apple.Safari', source_url: 'https://example.com/article' }} sourceDisplayNames={{ 'remote:host': 'Work Mac' }} />);
    expect(screen.getByText('Work Mac')).toBeInTheDocument();
    expect(screen.getByText('Safari')).toBeInTheDocument();
    expect(screen.getByText('https://example.com/article')).toBeInTheDocument();
    expect(await screen.findByTestId('source-app-icon')).toHaveAttribute('src', 'data:image/png;base64,com.apple.Safari');
  });

  it('omits the app icon when the app is not installed', async () => {
    const { commands } = await import('../bindings');
    render(<ClipDetail clip={{ ...baseClip, source_app: 'Arc', source_app_id: 'company.thebrowser.Browser' }} />);
    await waitFor(() => expect(commands.getAppIcon).toHaveBeenCalledWith('company.thebrowser.Browser'));
    expect(screen.queryByTestId('source-app-icon')).not.toBeInTheDocument();
    expect(screen.getByText('Arc')).toBeInTheDocument();
  });

  it('renders store-backed images even without a media path', () => {
    render(<ClipDetail clip={{ ...baseClip, id: 'image', content_type: 'image', content: '' }} />);
    expect(screen.getByRole('img')).toHaveAttribute('src', 'cinch://media/image');
  });

  it('shows saved pin notes without an empty note placeholder', () => {
    const { rerender } = render(<ClipDetail clip={{ ...baseClip, is_pinned: true, pin_note: 'Useful command' }} />);
    expect(screen.getByText('Useful command')).toBeInTheDocument();
    rerender(<ClipDetail clip={{ ...baseClip, is_pinned: true, pin_note: null }} />);
    expect(screen.queryByText('Note')).not.toBeInTheDocument();
  });

  it.each([
    { synced: false, sync_state: 'local', label: 'Local only' },
    { synced: false, sync_state: 'pending', label: 'Sending…' },
    { synced: true, sync_state: 'synced', label: 'Synced' },
  ])('shows $label delivery state', ({ synced, sync_state, label }) => {
    render(<ClipDetail clip={{ ...baseClip, synced, sync_state }} />);
    expect(screen.getByText(label)).toBeInTheDocument();
  });
});
