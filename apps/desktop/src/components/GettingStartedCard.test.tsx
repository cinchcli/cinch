import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { GettingStartedCard } from './GettingStartedCard';

const STORAGE_KEY = 'cinchGettingStartedDismissed';

describe('GettingStartedCard', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('renders heading and snippet', () => {
    render(<GettingStartedCard onCopySnippet={() => {}} onOpenDevices={() => {}} />);
    expect(screen.getByText(/You're signed in/i)).toBeInTheDocument();
    expect(screen.getByText('echo "hello cinch" | cinch push')).toBeInTheDocument();
  });

  it('invokes onCopySnippet with the exact snippet text when Copy is clicked', () => {
    const onCopySnippet = vi.fn();
    render(<GettingStartedCard onCopySnippet={onCopySnippet} onOpenDevices={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: /copy/i }));
    expect(onCopySnippet).toHaveBeenCalledWith('echo "hello cinch" | cinch push');
  });

  it('invokes onOpenDevices when the Devices link is clicked', () => {
    const onOpenDevices = vi.fn();
    render(<GettingStartedCard onCopySnippet={() => {}} onOpenDevices={onOpenDevices} />);
    fireEvent.click(screen.getByText(/Add machine/i));
    expect(onOpenDevices).toHaveBeenCalled();
  });

  it('persists dismissal to localStorage and unmounts when Dismiss is clicked', () => {
    const { container } = render(
      <GettingStartedCard onCopySnippet={() => {}} onOpenDevices={() => {}} />,
    );
    fireEvent.click(screen.getByRole('button', { name: /dismiss/i }));
    expect(localStorage.getItem(STORAGE_KEY)).toBe('1');
    expect(container.firstChild).toBeNull();
  });

  it('renders nothing if localStorage already has the dismissed marker on mount', () => {
    localStorage.setItem(STORAGE_KEY, '1');
    const { container } = render(
      <GettingStartedCard onCopySnippet={() => {}} onOpenDevices={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });
});
