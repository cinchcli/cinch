import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { GettingStartedCard } from './GettingStartedCard';

const STORAGE_KEY = 'cinchGettingStartedDismissed';

describe('GettingStartedCard', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('renders heading and snippet', () => {
    render(<GettingStartedCard onCopySnippet={() => {}} />);
    expect(screen.getByText(/You're signed in/i)).toBeInTheDocument();
    expect(screen.getByText('echo "hello cinch" | cinch push')).toBeInTheDocument();
  });

  it('invokes onCopySnippet with the exact snippet text when Copy is clicked', () => {
    const onCopySnippet = vi.fn();
    render(<GettingStartedCard onCopySnippet={onCopySnippet} />);
    fireEvent.click(screen.getAllByRole('button', { name: /copy/i })[0]);
    expect(onCopySnippet).toHaveBeenCalledWith('echo "hello cinch" | cinch push');
  });

  it('renders the SSH pair snippet', () => {
    const { getByText } = render(
      <GettingStartedCard onCopySnippet={vi.fn()} />,
    );
    expect(getByText('cinch pair user@host')).toBeInTheDocument();
  });

  it('copies the SSH pair snippet when its Copy button is clicked', () => {
    const onCopy = vi.fn();
    const { getAllByLabelText } = render(
      <GettingStartedCard onCopySnippet={onCopy} />,
    );
    // Two Copy buttons now: one for the local push snippet, one for the SSH snippet.
    const copyButtons = getAllByLabelText('Copy');
    expect(copyButtons).toHaveLength(2);
    copyButtons[1].click(); // second Copy = SSH snippet
    expect(onCopy).toHaveBeenCalledWith('cinch pair user@host');
  });

  it('renders the cask command for installing on another Mac', () => {
    const { getByText } = render(
      <GettingStartedCard onCopySnippet={vi.fn()} />,
    );
    expect(getByText('brew install --cask cinchcli/tap/cinchcli')).toBeInTheDocument();
  });

  it('persists dismissal to localStorage and unmounts when Dismiss is clicked', () => {
    const { container } = render(
      <GettingStartedCard onCopySnippet={() => {}} />,
    );
    fireEvent.click(screen.getByRole('button', { name: /dismiss/i }));
    expect(localStorage.getItem(STORAGE_KEY)).toBe('1');
    expect(container.firstChild).toBeNull();
  });

  it('renders nothing if localStorage already has the dismissed marker on mount', () => {
    localStorage.setItem(STORAGE_KEY, '1');
    const { container } = render(
      <GettingStartedCard onCopySnippet={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });
});
