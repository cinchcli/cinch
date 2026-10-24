import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { ClipList } from './ClipList';
import type { LocalClip } from '../bindings';

const NOW = 1_777_614_529; // matches our visual companion timestamp roughly

const clip = (overrides: Partial<LocalClip>): LocalClip => {
  const createdAt = overrides.created_at ?? NOW - 60;
  return {
    id: 'c1',
    content: 'hello world',
    content_type: 'text',
    byte_size: 11,
    source: 'local',
    source_app_id: null,
    source_app: null,
    source_url: null,
    created_at: createdAt,
    is_pinned: false,
    pin_note: null,
    media_path: null,
    user_id: 'u1',
    label: '',
    synced: false,
    sync_state: 'synced',
    received_at: overrides.received_at ?? createdAt,
    ...overrides,
  };
};

describe('ClipList', () => {
  it('renders empty state when no clips and no query', () => {
    render(
      <ClipList clips={[]} selected={null} onSelect={() => {}} onCopy={() => {}}
                query="" deviceNicknames={{}} now={NOW} />
    );
    expect(screen.getByText(/no clips/i)).toBeInTheDocument();
  });

  it('renders search-miss empty state when query and no clips', () => {
    render(
      <ClipList clips={[]} selected={null} onSelect={() => {}} onCopy={() => {}}
                query="foo" deviceNicknames={{}} now={NOW} />
    );
    expect(screen.getByText(/no results/i)).toBeInTheDocument();
    expect(screen.getByText(/foo/)).toBeInTheDocument();
  });

  it('groups clips into time bucket sections', () => {
    const clips = [
      clip({ id: 'a', created_at: NOW - 60 }),
      clip({ id: 'b', created_at: NOW - 86400 - 100 }),
    ];
    render(
      <ClipList clips={clips} selected={null} onSelect={() => {}} onCopy={() => {}}
                query="" deviceNicknames={{}} now={NOW} />
    );
    expect(screen.getByText('Today')).toBeInTheDocument();
    expect(screen.getByText('Yesterday')).toBeInTheDocument();
  });

  it('groups copied-again historical clips by received_at recency', () => {
    render(
      <ClipList
        clips={[clip({ id: 'old', created_at: NOW - 86400 * 30, received_at: NOW - 60 })]}
        selected={null}
        onSelect={() => {}}
        onCopy={() => {}}
        query=""
        deviceNicknames={{}}
        now={NOW}
      />
    );
    expect(screen.getByText('Today')).toBeInTheDocument();
    expect(screen.queryByText('Older')).not.toBeInTheDocument();
  });

  it('marks the selected clip with aria-current', () => {
    const c = clip({ id: 'a' });
    render(
      <ClipList clips={[c]} selected={c} onSelect={() => {}} onCopy={() => {}}
                query="" deviceNicknames={{}} now={NOW} />
    );
    const row = screen.getByRole('button', { name: /hello world/i });
    expect(row).toHaveAttribute('aria-current', 'true');
  });

  it('calls onSelect when row clicked', () => {
    const c = clip({ id: 'a' });
    const onSelect = vi.fn();
    render(
      <ClipList clips={[c]} selected={null} onSelect={onSelect} onCopy={() => {}}
                query="" deviceNicknames={{}} now={NOW} />
    );
    fireEvent.click(screen.getByRole('button', { name: /hello world/i }));
    expect(onSelect).toHaveBeenCalledWith(c);
  });

  it('calls onCopy when row double-clicked', () => {
    const c = clip({ id: 'a' });
    const onCopy = vi.fn();
    render(
      <ClipList clips={[c]} selected={null} onSelect={() => {}} onCopy={onCopy}
                query="" deviceNicknames={{}} now={NOW} />
    );
    fireEvent.doubleClick(screen.getByRole('button', { name: /hello world/i }));
    expect(onCopy).toHaveBeenCalledWith(c);
  });

  it('keeps provenance in the row tooltip without a metadata line', () => {
    const c = clip({ source: 'remote:host-x' });
    render(<ClipList clips={[c]} selected={null} onSelect={vi.fn()} onCopy={vi.fn()} query="" deviceNicknames={{ 'remote:host-x': 'Work Mac' }} />);
    expect(screen.getByRole('button', { name: c.content })).toHaveAttribute('title', 'Work Mac');
    expect(screen.queryByText('Work Mac')).not.toBeInTheDocument();
  });

  it('shows a pin indicator when the clip is pinned', () => {
    const c = clip({ id: 'a', is_pinned: true });
    render(
      <ClipList clips={[c]} selected={null} onSelect={() => {}} onCopy={() => {}}
                query="" deviceNicknames={{}} now={NOW} />
    );
    expect(screen.getByTitle('Pinned')).toBeInTheDocument();
  });

  it('hides the pin indicator when the clip is not pinned', () => {
    const c = clip({ id: 'a', is_pinned: false });
    render(
      <ClipList clips={[c]} selected={null} onSelect={() => {}} onCopy={() => {}}
                query="" deviceNicknames={{}} now={NOW} />
    );
    expect(screen.queryByTitle('Pinned')).not.toBeInTheDocument();
  });

  it('renders image preview for an image clip with no media_path (store-backed)', () => {
    const c = clip({ id: 'img-row', content_type: 'image', byte_size: 245760, content: '' });
    render(
      <ClipList clips={[c]} selected={null} onSelect={() => {}} onCopy={() => {}}
                query="" deviceNicknames={{}} now={NOW} />
    );
    // media_path defaults to null via the factory — image preview must still render
    expect(screen.getByText(/Image \(240\.0 KB\)/)).toBeInTheDocument();
  });

  it('renders a labeled placeholder for a whitespace-only clip instead of a blank row', () => {
    const c = clip({ id: 'blank', content: '\n', byte_size: 1 });
    render(
      <ClipList clips={[c]} selected={null} onSelect={() => {}} onCopy={() => {}}
                query="" deviceNicknames={{}} now={NOW} />
    );
    const preview = screen.getByRole('button', { name: /Blank/ });
    // A lone newline would otherwise trim to '' and render an invisible row.
    expect(preview.textContent).toContain('Blank');
  });

  it('preserves chronological placement of pinned clips', () => {
    const clips = [clip({ id: 'recent', content: 'recent clip' }), clip({ id: 'pin', content: 'older pin', is_pinned: true })];
    render(<ClipList clips={clips} selected={null} onSelect={vi.fn()} onCopy={vi.fn()} query="" />);
    expect(screen.getAllByRole('button').map((row) => row.getAttribute('data-id'))).toEqual(['recent', 'pin']);
    expect(screen.queryByText('Pinned')).not.toBeInTheDocument();
  });

  it('shows a Sending… badge for pending clips', () => {
    const c = clip({ id: 'p1', sync_state: 'pending' });
    render(
      <ClipList clips={[c]} selected={null} onSelect={() => {}} onCopy={() => {}}
                query="" deviceNicknames={{}} now={NOW} />
    );
    expect(screen.getByText('Sending…')).toBeInTheDocument();
  });

  it('leaves completed sync metadata to the preview', () => {
    render(<ClipList clips={[clip({ sync_state: 'synced' })]} selected={null} onSelect={vi.fn()} onCopy={vi.fn()} query="" />);
    expect(screen.queryByText('Sent')).not.toBeInTheDocument();
    expect(screen.getAllByRole('button')).toHaveLength(1);
  });

  it('renders pinned results without a permanent group heading', () => {
    render(<ClipList clips={[clip({ is_pinned: true })]} selected={null} onSelect={vi.fn()} onCopy={vi.fn()} query="" pinnedOnly />);
    expect(screen.queryByText('Today')).not.toBeInTheDocument();
    expect(screen.queryByText('Pinned')).not.toBeInTheDocument();
  });
});
