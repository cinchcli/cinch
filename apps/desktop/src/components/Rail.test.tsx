import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { Rail } from './Rail';

describe('Rail', () => {
  it('renders 4 icon buttons (Inbox, Pinned, Devices, Settings)', () => {
    render(<Rail active="inbox" onSelect={() => {}} onOpenSettings={() => {}} />);
    expect(screen.getByLabelText(/inbox/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/pinned/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/devices/i)).toBeInTheDocument();
    expect(screen.getByLabelText(/settings/i)).toBeInTheDocument();
  });

  it('marks the active item with aria-current="page"', () => {
    render(<Rail active="pinned" onSelect={() => {}} onOpenSettings={() => {}} />);
    expect(screen.getByLabelText(/pinned/i)).toHaveAttribute('aria-current', 'page');
    expect(screen.getByLabelText(/inbox/i)).not.toHaveAttribute('aria-current');
  });

  it('calls onSelect with the panel id when an icon is clicked', () => {
    const onSelect = vi.fn();
    render(<Rail active="inbox" onSelect={onSelect} onOpenSettings={() => {}} />);
    fireEvent.click(screen.getByLabelText(/devices/i));
    expect(onSelect).toHaveBeenCalledWith('devices');
  });

  it('calls onOpenSettings when the gear is clicked (not onSelect)', () => {
    const onSelect = vi.fn();
    const onOpenSettings = vi.fn();
    render(<Rail active="inbox" onSelect={onSelect} onOpenSettings={onOpenSettings} />);
    fireEvent.click(screen.getByLabelText(/settings/i));
    expect(onOpenSettings).toHaveBeenCalledTimes(1);
    expect(onSelect).not.toHaveBeenCalled();
  });
});
