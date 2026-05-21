import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));

import { EmptyState } from './EmptyState';

describe('EmptyState', () => {
  it('renders "Copy anything to start." heading for no-clips variant', () => {
    render(<EmptyState variant="no-clips" />);
    expect(screen.getByText('Copy anything to start.')).toBeInTheDocument();
  });

  it('renders "Clips appear here instantly." body for no-clips variant', () => {
    render(<EmptyState variant="no-clips" />);
    expect(screen.getByText('Clips appear here instantly.')).toBeInTheDocument();
  });

  it('renders search query in heading for search-miss variant', () => {
    render(<EmptyState variant="search-miss" query="hello" />);
    expect(screen.getByText('No clips match \u201Chello\u201D.')).toBeInTheDocument();
  });

  it('renders "Try a shorter word or clear the search." for search-miss body', () => {
    render(<EmptyState variant="search-miss" query="hello" />);
    expect(screen.getByText('Try a shorter word or clear the search.')).toBeInTheDocument();
  });

  it('uses C.t1 for heading and C.t2 for body text', () => {
    const { container } = render(<EmptyState variant="no-clips" />);
    const heading = container.querySelector('[data-testid="empty-heading"]');
    const body = container.querySelector('[data-testid="empty-body"]');
    expect(heading).toBeInTheDocument();
    expect(body).toBeInTheDocument();
    const headingStyle = heading!.getAttribute('style') || '';
    const bodyStyle = body!.getAttribute('style') || '';
    expect(headingStyle).toContain('var(--text-primary)');
    expect(bodyStyle).toContain('var(--text-muted)');
  });

  it('centers content vertically and horizontally', () => {
    const { container } = render(<EmptyState variant="no-clips" />);
    const wrapper = container.firstElementChild as HTMLElement;
    expect(wrapper).toHaveStyle({ display: 'flex', alignItems: 'center', justifyContent: 'center' });
  });
});
