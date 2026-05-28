import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { ClipCard } from './ClipCard';
import type { LocalClip } from '../bindings';

const textClip: LocalClip = {
  id: 'clip-1',
  user_id: 'user-1',
  content: 'Hello world this is a test of clip card preview text rendering',
  content_type: 'text',
  source: 'local',
  label: '',
  byte_size: 62,
  media_path: null,
  created_at: Math.floor(Date.now() / 1000) - 120,
  synced: false,
};

const imageClip: LocalClip = {
  id: 'clip-img-1',
  user_id: 'user-1',
  content: '',
  content_type: 'image',
  source: 'local',
  label: 'screenshot.png',
  byte_size: 245760,
  media_path: null,
  created_at: Math.floor(Date.now() / 1000) - 300,
  synced: false,
};

const binaryClip: LocalClip = {
  id: 'clip-bin-1',
  user_id: 'user-1',
  content: '',
  content_type: 'application/pdf',
  source: 'local',
  label: 'document.pdf',
  byte_size: 1048576,
  media_path: null,
  created_at: Math.floor(Date.now() / 1000) - 600,
  synced: false,
};

const defaultProps = {
  selected: false,
  onCopy: vi.fn(),
  onDelete: vi.fn(),
  onClick: vi.fn(),
};

describe('ClipCard', () => {
  describe('text variant', () => {
    it('renders clip preview text with CSS truncation', () => {
      const { container } = render(<ClipCard clip={textClip} {...defaultProps} />);
      const card = container.querySelector('[data-id="clip-1"]');
      expect(card).toBeInTheDocument();
      expect(card).toHaveAttribute('role', 'button');
      expect(screen.getByText(/Hello world/)).toBeInTheDocument();
    });

    it('renders type glyph in 32px-wide left column', () => {
      const { container } = render(<ClipCard clip={textClip} {...defaultProps} />);
      const glyphCol = container.querySelector('[data-testid="type-glyph"]');
      expect(glyphCol).toBeInTheDocument();
      expect(glyphCol).toHaveStyle({ width: '32px' });
    });

    it('renders a color swatch instead of the text glyph for standalone color clips', () => {
      const { container } = render(<ClipCard clip={{ ...textClip, content: 'rgb(255, 0, 0)' }} {...defaultProps} />);
      expect(container.querySelector('[data-testid="color-swatch"]')).toBeInTheDocument();
      expect(container.querySelector('[data-testid="type-glyph"]')).not.toBeInTheDocument();
      expect(screen.getByText('rgb(255, 0, 0)')).toBeInTheDocument();
    });

    it('renders timestamp in JetBrains Mono with tabular-nums', () => {
      const { container } = render(<ClipCard clip={textClip} {...defaultProps} />);
      const timestamp = container.querySelector('[data-testid="timestamp"]');
      expect(timestamp).toBeInTheDocument();
      const style = timestamp!.getAttribute('style') || '';
      expect(style).toContain('var(--font-mono)');
      expect(style).toContain('tabular-nums');
    });

    it('does not render key-cap badge', () => {
      render(<ClipCard clip={textClip} {...defaultProps} />);
      expect(screen.queryByText('Plain')).not.toBeInTheDocument();
    });

    it('shows copy and delete action buttons with aria-labels', () => {
      render(<ClipCard clip={textClip} {...defaultProps} />);
      expect(screen.getByLabelText('Copy clip')).toBeInTheDocument();
      expect(screen.getByLabelText('Delete clip')).toBeInTheDocument();
    });

    it('applies selected styles with accent left bar when selected=true', () => {
      const { container } = render(<ClipCard clip={textClip} {...defaultProps} selected={true} />);
      const card = container.querySelector('[data-id="clip-1"]');
      expect(card).toHaveAttribute('aria-selected', 'true');
    });
  });

  describe('image variant', () => {
    it('renders 48x48 thumbnail with correct src for image content_type', () => {
      const { container } = render(<ClipCard clip={imageClip} {...defaultProps} />);
      const img = container.querySelector('img');
      expect(img).toBeInTheDocument();
      expect(img).toHaveAttribute('src', 'cinch://media/clip-img-1');
      expect(img).toHaveStyle({ width: '48px', height: '48px' });
    });

    it('renders file-size text using formatBytes', () => {
      render(<ClipCard clip={imageClip} {...defaultProps} />);
      expect(screen.getByText(/240\.0 KB/)).toBeInTheDocument();
    });

    it('renders file-type icon for non-image binary content', () => {
      const { container } = render(<ClipCard clip={binaryClip} {...defaultProps} />);
      const binarySlot = container.querySelector('[data-testid="binary-slot"]');
      expect(binarySlot).toBeInTheDocument();
      expect(binarySlot).toHaveStyle({ width: '48px', height: '48px' });
    });

    it('renders error state when image download fails', () => {
      const { container } = render(<ClipCard clip={imageClip} {...defaultProps} />);
      const img = container.querySelector('img');
      expect(img).toBeInTheDocument();
      fireEvent.error(img!);
      expect(screen.getByText('Preview unavailable')).toBeInTheDocument();
    });
  });
});
