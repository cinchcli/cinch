import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { createRef } from 'react';
import { SearchBar, type DeviceOption, type AppOption } from './SearchBar';
import type { ClipFilter } from '../lib/clipFilters';

const DEFAULT_DEVICES: DeviceOption[] = [
  { source: 'remote:macbook', label: 'MacBook',    count: 87, colorSlot: 'mint' },
  { source: 'remote:iphone',  label: 'iPhone',     count: 31, colorSlot: 'sky' },
  { source: 'remote:linux',   label: 'Linux Box',  count: 24, colorSlot: 'amber' },
];

const DEFAULT_APPS: AppOption[] = [
  { id: 'com.apple.Safari',     label: 'Safari',   count: 42 },
  { id: 'com.microsoft.VSCode', label: 'Code',     count: 17 },
  { id: 'com.apple.Terminal',   label: 'Terminal', count: 9 },
];

type ThemeMode = 'light' | 'dark' | 'system';

function renderBar(overrides: Partial<{
  value: string;
  onChange: (s: string) => void;
  activeFilter: ClipFilter;
  onFilterChange: (f: ClipFilter) => void;
  deviceOptions: DeviceOption[];
  selectedSource: string | null;
  onSourceChange: (s: string | null) => void;
  appOptions: AppOption[];
  selectedApp: string | null;
  onAppChange: (a: string | null) => void;
  themeMode: ThemeMode;
  onSetThemeMode: (m: ThemeMode) => void;
}> = {}) {
  const onChange = overrides.onChange ?? vi.fn();
  const onFilterChange = overrides.onFilterChange ?? vi.fn();
  const onSourceChange = overrides.onSourceChange ?? vi.fn();
  const onAppChange = overrides.onAppChange ?? vi.fn();
  const onSetThemeMode = overrides.onSetThemeMode ?? vi.fn();
  const result = render(
    <SearchBar
      ref={createRef()}
      value={overrides.value ?? ''}
      onChange={onChange}
      onClear={vi.fn()}
      themeMode={overrides.themeMode ?? 'dark'}
      onSetThemeMode={onSetThemeMode}
      onMouseDown={vi.fn()}
      activeFilter={overrides.activeFilter ?? 'all'}
      onFilterChange={onFilterChange}
      deviceOptions={overrides.deviceOptions ?? DEFAULT_DEVICES}
      selectedSource={overrides.selectedSource ?? null}
      onSourceChange={onSourceChange}
      appOptions={overrides.appOptions ?? DEFAULT_APPS}
      selectedApp={overrides.selectedApp ?? null}
      onAppChange={onAppChange}
    />
  );
  // The aria-label is stable regardless of the active filter mode, so this is
  // a safe handle even after a sigil opens a dropdown (which changes the
  // placeholder).
  const input = screen.getByLabelText('Search clips') as HTMLInputElement;
  return { ...result, input, onChange, onFilterChange, onSourceChange, onAppChange, onSetThemeMode };
}

describe('SearchBar', () => {
  it('renders search input', () => {
    renderBar();
    expect(screen.getByPlaceholderText(/search clips/i)).toBeInTheDocument();
  });

  describe('filter dropdown (# trigger)', () => {
    it('opens when # is typed in the input', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '#' } });
      expect(screen.getByTestId('filter-dropdown')).toBeInTheDocument();
    });

    it('shows all five filter options when the query is empty', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '#' } });
      expect(screen.getByTestId('filter-option-all')).toBeInTheDocument();
      expect(screen.getByTestId('filter-option-text')).toBeInTheDocument();
      expect(screen.getByTestId('filter-option-image')).toBeInTheDocument();
      expect(screen.getByTestId('filter-option-code')).toBeInTheDocument();
      expect(screen.getByTestId('filter-option-url')).toBeInTheDocument();
    });

    it('strips # from the value passed to onChange', () => {
      const { input, onChange } = renderBar({ value: 'hello' });
      fireEvent.change(input, { target: { value: 'hello#' } });
      expect(onChange).toHaveBeenCalledWith('hello');
    });

    it('closes on Escape without calling onFilterChange', () => {
      const { input, onFilterChange } = renderBar();
      fireEvent.change(input, { target: { value: '#' } });
      fireEvent.keyDown(input, { key: 'Escape' });
      expect(screen.queryByTestId('filter-dropdown')).not.toBeInTheDocument();
      expect(onFilterChange).not.toHaveBeenCalled();
    });

    it('narrows to matching items (non-matches absent) as the query is typed', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '#c' } });
      // 'code' matches 'c'; the others are removed from the DOM, not dimmed.
      expect(screen.getByTestId('filter-option-code')).toBeInTheDocument();
      expect(screen.getByTestId('filter-option-code')).toHaveAttribute('aria-selected', 'true');
      expect(screen.queryByTestId('filter-option-text')).not.toBeInTheDocument();
      expect(screen.queryByTestId('filter-option-all')).not.toBeInTheDocument();
    });

    it('Enter selects the highlighted filter and closes dropdown', () => {
      const { input, onFilterChange } = renderBar();
      fireEvent.change(input, { target: { value: '#c' } });
      fireEvent.keyDown(input, { key: 'Enter' });
      expect(onFilterChange).toHaveBeenCalledWith('code');
      expect(screen.queryByTestId('filter-dropdown')).not.toBeInTheDocument();
    });

    it('ArrowDown moves highlight to the next matching item', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '#' } });
      // initial highlight is 'all' (CLIP_FILTERS[0]); ArrowDown → 'text'
      fireEvent.keyDown(input, { key: 'ArrowDown' });
      expect(screen.getByTestId('filter-option-text')).toHaveAttribute('aria-selected', 'true');
      expect(screen.getByTestId('filter-option-all')).toHaveAttribute('aria-selected', 'false');
    });

    it('clicking a dropdown item selects it', () => {
      const { input, onFilterChange } = renderBar();
      fireEvent.change(input, { target: { value: '#' } });
      fireEvent.mouseDown(screen.getByTestId('filter-option-image'));
      expect(onFilterChange).toHaveBeenCalledWith('image');
    });

    it('selecting "all" calls onFilterChange with "all"', () => {
      const { input, onFilterChange } = renderBar({ activeFilter: 'image' });
      fireEvent.change(input, { target: { value: '#' } });
      fireEvent.mouseDown(screen.getByTestId('filter-option-all'));
      expect(onFilterChange).toHaveBeenCalledWith('all');
    });
  });

  describe('filter chip', () => {
    it('shows chip when activeFilter is not "all"', () => {
      renderBar({ activeFilter: 'image' });
      expect(screen.getByTestId('filter-chip')).toBeInTheDocument();
      expect(screen.getByTestId('filter-chip')).toHaveTextContent('image');
    });

    it('does not show chip when activeFilter is "all"', () => {
      renderBar({ activeFilter: 'all' });
      expect(screen.queryByTestId('filter-chip')).not.toBeInTheDocument();
    });

    it('clicking ✕ calls onFilterChange with "all"', () => {
      const { onFilterChange } = renderBar({ activeFilter: 'code' });
      fireEvent.click(screen.getByTestId('filter-chip-x'));
      expect(onFilterChange).toHaveBeenCalledWith('all');
    });

    it('clicking chip body (not ✕) reopens dropdown', () => {
      renderBar({ activeFilter: 'image' });
      fireEvent.click(screen.getByTestId('filter-chip'));
      expect(screen.getByTestId('filter-dropdown')).toBeInTheDocument();
    });

    it('dropdown pre-highlights current filter when reopened from chip click', () => {
      renderBar({ activeFilter: 'code' });
      fireEvent.click(screen.getByTestId('filter-chip'));
      expect(screen.getByTestId('filter-option-code')).toHaveAttribute('aria-selected', 'true');
    });

    it('Backspace on empty input with active filter calls onFilterChange("all")', () => {
      const { input, onFilterChange } = renderBar({ value: '', activeFilter: 'url' });
      fireEvent.keyDown(input, { key: 'Backspace' });
      expect(onFilterChange).toHaveBeenCalledWith('all');
    });

    it('placeholder shows # and @ hints when no filter or source is active', () => {
      renderBar({ activeFilter: 'all' });
      expect(screen.getByPlaceholderText(/# type, @ device/i)).toBeInTheDocument();
    });

    it('placeholder is empty when a filter chip is active', () => {
      renderBar({ activeFilter: 'text' });
      expect(screen.getByLabelText('Search clips')).toHaveAttribute('placeholder', '');
    });
  });

  describe('device dropdown (@ trigger)', () => {
    it('opens when @ is typed in the input', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '@' } });
      expect(screen.getByTestId('device-dropdown')).toBeInTheDocument();
    });

    it('lists every device with no "all devices" entry', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '@' } });
      expect(screen.queryByTestId('device-option-all')).not.toBeInTheDocument();
      expect(screen.getByTestId('device-option-remote:macbook')).toBeInTheDocument();
      expect(screen.getByTestId('device-option-remote:iphone')).toBeInTheDocument();
      expect(screen.getByTestId('device-option-remote:linux')).toBeInTheDocument();
    });

    it('strips @ from the value passed to onChange', () => {
      const { input, onChange } = renderBar({ value: 'hello' });
      fireEvent.change(input, { target: { value: 'hello@' } });
      expect(onChange).toHaveBeenCalledWith('hello');
    });

    it('narrows to matching devices by label prefix', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '@i' } });
      expect(screen.getByTestId('device-option-remote:iphone')).toBeInTheDocument();
      expect(screen.getByTestId('device-option-remote:iphone')).toHaveAttribute('aria-selected', 'true');
      expect(screen.queryByTestId('device-option-remote:macbook')).not.toBeInTheDocument();
    });

    it('Enter selects highlighted device and closes dropdown', () => {
      const { input, onSourceChange } = renderBar();
      fireEvent.change(input, { target: { value: '@i' } });
      fireEvent.keyDown(input, { key: 'Enter' });
      expect(onSourceChange).toHaveBeenCalledWith('remote:iphone');
      expect(screen.queryByTestId('device-dropdown')).not.toBeInTheDocument();
    });

    it('ArrowDown moves highlight to the next device', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '@' } });
      // initial highlight is the first device (MacBook); ArrowDown → iPhone
      fireEvent.keyDown(input, { key: 'ArrowDown' });
      expect(screen.getByTestId('device-option-remote:iphone')).toHaveAttribute('aria-selected', 'true');
      expect(screen.getByTestId('device-option-remote:macbook')).toHaveAttribute('aria-selected', 'false');
    });

    it('clicking a device option selects it', () => {
      const { input, onSourceChange } = renderBar();
      fireEvent.change(input, { target: { value: '@' } });
      fireEvent.mouseDown(screen.getByTestId('device-option-remote:linux'));
      expect(onSourceChange).toHaveBeenCalledWith('remote:linux');
    });

    it('Escape closes the device dropdown without calling onSourceChange', () => {
      const { input, onSourceChange } = renderBar();
      fireEvent.change(input, { target: { value: '@' } });
      fireEvent.keyDown(input, { key: 'Escape' });
      expect(screen.queryByTestId('device-dropdown')).not.toBeInTheDocument();
      expect(onSourceChange).not.toHaveBeenCalled();
    });

    it('shows empty-state when there are no devices', () => {
      const { input } = renderBar({ deviceOptions: [] });
      fireEvent.change(input, { target: { value: '@' } });
      expect(screen.getByTestId('device-option-empty')).toBeInTheDocument();
    });
  });

  describe('device chip', () => {
    it('shows chip when selectedSource is set', () => {
      renderBar({ selectedSource: 'remote:macbook' });
      const chip = screen.getByTestId('device-chip');
      expect(chip).toBeInTheDocument();
      expect(chip).toHaveTextContent('MacBook');
    });

    it('does not show chip when selectedSource is null', () => {
      renderBar({ selectedSource: null });
      expect(screen.queryByTestId('device-chip')).not.toBeInTheDocument();
    });

    it('clicking ✕ calls onSourceChange with null', () => {
      const { onSourceChange } = renderBar({ selectedSource: 'remote:iphone' });
      fireEvent.click(screen.getByTestId('device-chip-x'));
      expect(onSourceChange).toHaveBeenCalledWith(null);
    });

    it('clicking chip body (not ✕) reopens device dropdown', () => {
      renderBar({ selectedSource: 'remote:macbook' });
      fireEvent.click(screen.getByTestId('device-chip'));
      expect(screen.getByTestId('device-dropdown')).toBeInTheDocument();
    });

    it('Backspace on empty input with active source clears it', () => {
      const { input, onSourceChange } = renderBar({ value: '', selectedSource: 'remote:linux' });
      fireEvent.keyDown(input, { key: 'Backspace' });
      expect(onSourceChange).toHaveBeenCalledWith(null);
    });

    it('Backspace on empty input clears type chip first when both are set', () => {
      const { input, onSourceChange, onFilterChange } = renderBar({
        value: '',
        activeFilter: 'code',
        selectedSource: 'remote:macbook',
      });
      fireEvent.keyDown(input, { key: 'Backspace' });
      expect(onFilterChange).toHaveBeenCalledWith('all');
      expect(onSourceChange).not.toHaveBeenCalled();
    });

    it('renders source chip alongside type chip when both are active', () => {
      renderBar({ selectedSource: 'remote:macbook', activeFilter: 'image' });
      expect(screen.getByTestId('device-chip')).toBeInTheDocument();
      expect(screen.getByTestId('filter-chip')).toBeInTheDocument();
    });

    it('unknown selectedSource (not in options) does not render a chip', () => {
      renderBar({ selectedSource: 'remote:ghost' });
      expect(screen.queryByTestId('device-chip')).not.toBeInTheDocument();
    });
  });

  describe('app dropdown (> trigger)', () => {
    it('opens when > is typed in the input', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '>' } });
      expect(screen.getByTestId('app-dropdown')).toBeInTheDocument();
    });

    it('lists every app you have copied from', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '>' } });
      expect(screen.getByTestId('app-option-com.apple.Safari')).toBeInTheDocument();
      expect(screen.getByTestId('app-option-com.microsoft.VSCode')).toBeInTheDocument();
      expect(screen.getByTestId('app-option-com.apple.Terminal')).toBeInTheDocument();
    });

    it('strips > from the value passed to onChange', () => {
      const { input, onChange } = renderBar({ value: 'hello' });
      fireEvent.change(input, { target: { value: 'hello>' } });
      expect(onChange).toHaveBeenCalledWith('hello');
    });

    it('narrows to matching apps by label prefix', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '>t' } });
      expect(screen.getByTestId('app-option-com.apple.Terminal')).toBeInTheDocument();
      expect(screen.getByTestId('app-option-com.apple.Terminal')).toHaveAttribute('aria-selected', 'true');
      expect(screen.queryByTestId('app-option-com.apple.Safari')).not.toBeInTheDocument();
    });

    it('Enter selects highlighted app and closes dropdown', () => {
      const { input, onAppChange } = renderBar();
      fireEvent.change(input, { target: { value: '>t' } });
      fireEvent.keyDown(input, { key: 'Enter' });
      expect(onAppChange).toHaveBeenCalledWith('com.apple.Terminal');
      expect(screen.queryByTestId('app-dropdown')).not.toBeInTheDocument();
    });

    it('ArrowDown moves highlight to the next app', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '>' } });
      // initial highlight is the first app (Safari); ArrowDown → Code
      fireEvent.keyDown(input, { key: 'ArrowDown' });
      expect(screen.getByTestId('app-option-com.microsoft.VSCode')).toHaveAttribute('aria-selected', 'true');
      expect(screen.getByTestId('app-option-com.apple.Safari')).toHaveAttribute('aria-selected', 'false');
    });

    it('clicking an app option selects it', () => {
      const { input, onAppChange } = renderBar();
      fireEvent.change(input, { target: { value: '>' } });
      fireEvent.mouseDown(screen.getByTestId('app-option-com.microsoft.VSCode'));
      expect(onAppChange).toHaveBeenCalledWith('com.microsoft.VSCode');
    });

    it('Escape closes the app dropdown without calling onAppChange', () => {
      const { input, onAppChange } = renderBar();
      fireEvent.change(input, { target: { value: '>' } });
      fireEvent.keyDown(input, { key: 'Escape' });
      expect(screen.queryByTestId('app-dropdown')).not.toBeInTheDocument();
      expect(onAppChange).not.toHaveBeenCalled();
    });

    it('shows empty-state when there are no apps', () => {
      const { input } = renderBar({ appOptions: [] });
      fireEvent.change(input, { target: { value: '>' } });
      expect(screen.getByTestId('app-option-empty')).toBeInTheDocument();
    });
  });

  describe('app chip', () => {
    it('shows chip when selectedApp is set', () => {
      renderBar({ selectedApp: 'com.apple.Safari' });
      const chip = screen.getByTestId('app-chip');
      expect(chip).toBeInTheDocument();
      expect(chip).toHaveTextContent('Safari');
    });

    it('does not show chip when selectedApp is null', () => {
      renderBar({ selectedApp: null });
      expect(screen.queryByTestId('app-chip')).not.toBeInTheDocument();
    });

    it('clicking ✕ calls onAppChange with null', () => {
      const { onAppChange } = renderBar({ selectedApp: 'com.microsoft.VSCode' });
      fireEvent.click(screen.getByTestId('app-chip-x'));
      expect(onAppChange).toHaveBeenCalledWith(null);
    });

    it('clicking chip body (not ✕) reopens app dropdown', () => {
      renderBar({ selectedApp: 'com.apple.Safari' });
      fireEvent.click(screen.getByTestId('app-chip'));
      expect(screen.getByTestId('app-dropdown')).toBeInTheDocument();
    });

    it('unknown selectedApp (not in options) does not render a chip', () => {
      renderBar({ selectedApp: 'com.ghost.App' });
      expect(screen.queryByTestId('app-chip')).not.toBeInTheDocument();
    });

    it('Backspace on empty clears app chip before device chip (no type)', () => {
      const { input, onAppChange, onSourceChange } = renderBar({
        value: '',
        selectedApp: 'com.apple.Safari',
        selectedSource: 'remote:macbook',
      });
      fireEvent.keyDown(input, { key: 'Backspace' });
      expect(onAppChange).toHaveBeenCalledWith(null);
      expect(onSourceChange).not.toHaveBeenCalled();
    });

    it('Backspace on empty clears type chip before app chip', () => {
      const { input, onFilterChange, onAppChange } = renderBar({
        value: '',
        activeFilter: 'code',
        selectedApp: 'com.apple.Safari',
      });
      fireEvent.keyDown(input, { key: 'Backspace' });
      expect(onFilterChange).toHaveBeenCalledWith('all');
      expect(onAppChange).not.toHaveBeenCalled();
    });

    it('placeholder shows the > app hint when nothing is active', () => {
      renderBar({ activeFilter: 'all', selectedSource: null, selectedApp: null });
      expect(screen.getByPlaceholderText(/> app/i)).toBeInTheDocument();
    });

    it('renders all three chips when type, app, and device are active', () => {
      renderBar({ activeFilter: 'code', selectedApp: 'com.apple.Safari', selectedSource: 'remote:macbook' });
      expect(screen.getByTestId('filter-chip')).toBeInTheDocument();
      expect(screen.getByTestId('app-chip')).toBeInTheDocument();
      expect(screen.getByTestId('device-chip')).toBeInTheDocument();
    });

    it('Backspace on empty clears the type chip first when all three are active', () => {
      const { input, onFilterChange, onAppChange, onSourceChange } = renderBar({
        value: '',
        activeFilter: 'code',
        selectedApp: 'com.apple.Safari',
        selectedSource: 'remote:macbook',
      });
      fireEvent.keyDown(input, { key: 'Backspace' });
      expect(onFilterChange).toHaveBeenCalledWith('all');
      expect(onAppChange).not.toHaveBeenCalled();
      expect(onSourceChange).not.toHaveBeenCalled();
    });
  });

  // ── New behavior: type-through (the query is visible in the input) ──────────
  describe('type-through query echo', () => {
    it('typing > then an app query shows the query in the input', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '>arc' } });
      expect(input).toHaveValue('arc');
      expect(screen.getByTestId('app-dropdown')).toBeInTheDocument();
    });

    it('typing # then a type query shows the query in the input', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '#ima' } });
      expect(input).toHaveValue('ima');
      expect(screen.getByTestId('filter-dropdown')).toBeInTheDocument();
    });

    it('typing @ then a device query shows the query in the input', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '@mac' } });
      expect(input).toHaveValue('mac');
      expect(screen.getByTestId('device-dropdown')).toBeInTheDocument();
    });

    it('the live query, not the pre-sigil clip-search text, is shown while in a mode', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '>sa' } });
      expect(input).toHaveValue('sa');
    });
  });

  describe('mode prefix pill', () => {
    it('is absent when no mode is active', () => {
      renderBar();
      expect(screen.queryByTestId('mode-pill')).not.toBeInTheDocument();
    });

    it('shows the app label when in app mode', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '>' } });
      expect(screen.getByTestId('mode-pill')).toHaveTextContent('app');
    });

    it('shows the device label when in device mode', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '@' } });
      expect(screen.getByTestId('mode-pill')).toHaveTextContent('device');
    });

    it('shows the type label when in type mode', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '#' } });
      expect(screen.getByTestId('mode-pill')).toHaveTextContent('type');
    });

    it('renders alongside a committed chip when reopening that filter via sigil', () => {
      const { input } = renderBar({ selectedApp: 'com.apple.Safari' });
      fireEvent.change(input, { target: { value: '>' } });
      expect(screen.getByTestId('app-chip')).toBeInTheDocument();
      expect(screen.getByTestId('mode-pill')).toBeInTheDocument();
    });
  });

  describe('commit clears the query and exits the mode', () => {
    it('Enter commits the highlighted app, clears the query, and exits', () => {
      const { input, onAppChange } = renderBar();
      fireEvent.change(input, { target: { value: '>co' } });
      fireEvent.keyDown(input, { key: 'Enter' });
      expect(onAppChange).toHaveBeenCalledWith('com.microsoft.VSCode');
      expect(input).toHaveValue('');
      expect(screen.queryByTestId('app-dropdown')).not.toBeInTheDocument();
      expect(screen.queryByTestId('mode-pill')).not.toBeInTheDocument();
    });

    it('clicking an app option clears the query and exits', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '>co' } });
      fireEvent.mouseDown(screen.getByTestId('app-option-com.microsoft.VSCode'));
      expect(input).toHaveValue('');
      expect(screen.queryByTestId('app-dropdown')).not.toBeInTheDocument();
    });
  });

  describe('exiting a mode', () => {
    it('Escape exits the mode without committing and clears the query', () => {
      const { input, onAppChange } = renderBar();
      fireEvent.change(input, { target: { value: '>arc' } });
      fireEvent.keyDown(input, { key: 'Escape' });
      expect(onAppChange).not.toHaveBeenCalled();
      expect(input).toHaveValue('');
      expect(screen.queryByTestId('app-dropdown')).not.toBeInTheDocument();
    });

    it('Backspace on an empty query exits the mode', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '>' } });
      fireEvent.keyDown(input, { key: 'Backspace' });
      expect(screen.queryByTestId('app-dropdown')).not.toBeInTheDocument();
      expect(screen.queryByTestId('mode-pill')).not.toBeInTheDocument();
    });

    it('Backspace on a non-empty query edits the query and stays in the mode', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '>cod' } });
      fireEvent.keyDown(input, { key: 'Backspace' });
      expect(input).toHaveValue('co');
      expect(screen.getByTestId('app-dropdown')).toBeInTheDocument();
    });
  });

  describe('single-mode invariant (no nesting)', () => {
    it('a sigil typed inside a mode is literal query text, not a mode switch', () => {
      const { input } = renderBar();
      // '@' enters device mode; the later '>' is literal query text.
      fireEvent.change(input, { target: { value: '@de>v' } });
      expect(screen.getByTestId('device-dropdown')).toBeInTheDocument();
      expect(screen.queryByTestId('app-dropdown')).not.toBeInTheDocument();
      expect(input).toHaveValue('de>v');
    });

    it('only one dropdown is ever open at a time', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '#' } });
      expect(screen.getByTestId('filter-dropdown')).toBeInTheDocument();
      expect(screen.queryByTestId('device-dropdown')).not.toBeInTheDocument();
      expect(screen.queryByTestId('app-dropdown')).not.toBeInTheDocument();
    });

    it('a sigil typed in a SECOND change (already in a mode) stays literal', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '@' } });   // enter device mode
      fireEvent.change(input, { target: { value: '>' } });   // typed while already in a mode
      expect(screen.getByTestId('device-dropdown')).toBeInTheDocument();
      expect(screen.queryByTestId('app-dropdown')).not.toBeInTheDocument();
      expect(input).toHaveValue('>');
    });
  });

  describe('highlight robustness', () => {
    it('Enter is a no-op when the query matches no rows', () => {
      const { input, onAppChange } = renderBar();
      fireEvent.change(input, { target: { value: '>zzz' } });
      fireEvent.keyDown(input, { key: 'Enter' });
      expect(onAppChange).not.toHaveBeenCalled();
      expect(screen.getByTestId('app-dropdown')).toBeInTheDocument();
    });

    it('resyncs the highlight to the top match when options change mid-mode', () => {
      const onAppChange = vi.fn();
      const base = {
        value: '', onChange: vi.fn(), onClear: vi.fn(),
        themeMode: 'dark' as const, onSetThemeMode: vi.fn(), onMouseDown: vi.fn(),
        activeFilter: 'all' as const, onFilterChange: vi.fn(),
        deviceOptions: DEFAULT_DEVICES, selectedSource: null, onSourceChange: vi.fn(),
        selectedApp: null, onAppChange,
      };
      const ref = createRef<HTMLInputElement>();
      const { rerender } = render(<SearchBar ref={ref} {...base} appOptions={DEFAULT_APPS} />);
      const input = screen.getByLabelText('Search clips');
      fireEvent.change(input, { target: { value: '>' } });
      expect(screen.getByTestId('app-option-com.apple.Safari')).toHaveAttribute('aria-selected', 'true');
      // The parent drops the highlighted app (Safari) while the dropdown is open.
      rerender(
        <SearchBar ref={ref} {...base} appOptions={[
          { id: 'com.apple.Terminal',   label: 'Terminal', count: 9 },
          { id: 'com.microsoft.VSCode', label: 'Code',     count: 17 },
        ]} />
      );
      expect(screen.queryByTestId('app-option-com.apple.Safari')).not.toBeInTheDocument();
      // The highlight does not vanish — it resyncs to the new top match.
      expect(screen.getByTestId('app-option-com.apple.Terminal')).toHaveAttribute('aria-selected', 'true');
    });
  });

  describe('clear button visibility', () => {
    it('hides the clear button while a filter mode is open', () => {
      const { input } = renderBar({ value: 'hello' });
      expect(screen.getByLabelText('Clear search')).toBeInTheDocument();
      fireEvent.change(input, { target: { value: 'hello>' } }); // pre-sigil text kept; mode opens
      expect(screen.queryByLabelText('Clear search')).not.toBeInTheDocument();
    });
  });

  describe('aria roles', () => {
    it('the dropdown is a listbox of options', () => {
      const { input } = renderBar();
      fireEvent.change(input, { target: { value: '>' } });
      expect(screen.getByTestId('app-dropdown')).toHaveAttribute('role', 'listbox');
      expect(screen.getByTestId('app-option-com.apple.Safari')).toHaveAttribute('role', 'option');
    });
  });

  describe('window-level keydown isolation', () => {
    // Regression: Enter / ArrowUp / ArrowDown inside an open dropdown must NOT
    // reach the global `window.addEventListener('keydown', …)` handler in
    // App.tsx, which would otherwise copy the selected clip (Enter) or move
    // clip selection (Arrows) at the same time as picking a dropdown option.
    function trackWindowKeydowns(keys: Set<string>) {
      const seen: string[] = [];
      const listener = (e: KeyboardEvent) => {
        if (keys.has(e.key)) seen.push(e.key);
      };
      window.addEventListener('keydown', listener);
      return {
        seen,
        cleanup: () => window.removeEventListener('keydown', listener),
      };
    }

    it('Enter inside device dropdown does not reach window listeners', () => {
      const tracker = trackWindowKeydowns(new Set(['Enter']));
      try {
        const { input } = renderBar();
        fireEvent.change(input, { target: { value: '@' } });
        fireEvent.keyDown(input, { key: 'Enter' });
        expect(tracker.seen).toEqual([]);
      } finally {
        tracker.cleanup();
      }
    });

    it('Enter inside filter dropdown does not reach window listeners', () => {
      const tracker = trackWindowKeydowns(new Set(['Enter']));
      try {
        const { input } = renderBar();
        fireEvent.change(input, { target: { value: '#' } });
        fireEvent.keyDown(input, { key: 'Enter' });
        expect(tracker.seen).toEqual([]);
      } finally {
        tracker.cleanup();
      }
    });

    it('Arrow keys inside device dropdown do not reach window listeners', () => {
      const tracker = trackWindowKeydowns(new Set(['ArrowDown', 'ArrowUp']));
      try {
        const { input } = renderBar();
        fireEvent.change(input, { target: { value: '@' } });
        fireEvent.keyDown(input, { key: 'ArrowDown' });
        fireEvent.keyDown(input, { key: 'ArrowUp' });
        expect(tracker.seen).toEqual([]);
      } finally {
        tracker.cleanup();
      }
    });

    it('Enter inside app dropdown does not reach window listeners', () => {
      const tracker = trackWindowKeydowns(new Set(['Enter']));
      try {
        const { input } = renderBar();
        fireEvent.change(input, { target: { value: '>' } });
        fireEvent.keyDown(input, { key: 'Enter' });
        expect(tracker.seen).toEqual([]);
      } finally {
        tracker.cleanup();
      }
    });

    it('Arrow keys inside app dropdown do not reach window listeners', () => {
      const tracker = trackWindowKeydowns(new Set(['ArrowDown', 'ArrowUp']));
      try {
        const { input } = renderBar();
        fireEvent.change(input, { target: { value: '>' } });
        fireEvent.keyDown(input, { key: 'ArrowDown' });
        fireEvent.keyDown(input, { key: 'ArrowUp' });
        expect(tracker.seen).toEqual([]);
      } finally {
        tracker.cleanup();
      }
    });

    it('Enter outside any dropdown still reaches window listeners', () => {
      // Sanity check: the isolation is scoped to open dropdowns. With no mode
      // active, Enter should bubble normally — App.tsx relies on this path to
      // copy the selected clip when the user hits Enter in the search field.
      const tracker = trackWindowKeydowns(new Set(['Enter']));
      try {
        const { input } = renderBar();
        fireEvent.keyDown(input, { key: 'Enter' });
        expect(tracker.seen).toEqual(['Enter']);
      } finally {
        tracker.cleanup();
      }
    });
  });

  describe('theme menu', () => {
    it('does not show the menu by default', () => {
      renderBar();
      expect(screen.queryByTestId('theme-menu')).not.toBeInTheDocument();
    });

    it('opens the menu when the theme button is clicked', () => {
      renderBar();
      fireEvent.click(screen.getByTestId('theme-toggle'));
      expect(screen.getByTestId('theme-menu')).toBeInTheDocument();
    });

    it('lists Light, Dark, and System options', () => {
      renderBar();
      fireEvent.click(screen.getByTestId('theme-toggle'));
      expect(screen.getByTestId('theme-option-light')).toBeInTheDocument();
      expect(screen.getByTestId('theme-option-dark')).toBeInTheDocument();
      expect(screen.getByTestId('theme-option-system')).toBeInTheDocument();
    });

    it('marks the active mode with aria-checked=true', () => {
      renderBar({ themeMode: 'system' });
      fireEvent.click(screen.getByTestId('theme-toggle'));
      expect(screen.getByTestId('theme-option-system')).toHaveAttribute('aria-checked', 'true');
      expect(screen.getByTestId('theme-option-light')).toHaveAttribute('aria-checked', 'false');
      expect(screen.getByTestId('theme-option-dark')).toHaveAttribute('aria-checked', 'false');
    });

    it('selecting an option calls onSetThemeMode and closes the menu', () => {
      const { onSetThemeMode } = renderBar({ themeMode: 'dark' });
      fireEvent.click(screen.getByTestId('theme-toggle'));
      fireEvent.mouseDown(screen.getByTestId('theme-option-system'));
      expect(onSetThemeMode).toHaveBeenCalledWith('system');
      expect(screen.queryByTestId('theme-menu')).not.toBeInTheDocument();
    });

    it('clicking the toggle a second time closes the menu', () => {
      renderBar();
      const toggle = screen.getByTestId('theme-toggle');
      fireEvent.click(toggle);
      expect(screen.getByTestId('theme-menu')).toBeInTheDocument();
      fireEvent.click(toggle);
      expect(screen.queryByTestId('theme-menu')).not.toBeInTheDocument();
    });

    it('Escape closes the menu', () => {
      renderBar();
      fireEvent.click(screen.getByTestId('theme-toggle'));
      expect(screen.getByTestId('theme-menu')).toBeInTheDocument();
      fireEvent.keyDown(document, { key: 'Escape' });
      expect(screen.queryByTestId('theme-menu')).not.toBeInTheDocument();
    });

    it('clicking outside the menu closes it', () => {
      renderBar();
      fireEvent.click(screen.getByTestId('theme-toggle'));
      expect(screen.getByTestId('theme-menu')).toBeInTheDocument();
      fireEvent.mouseDown(document.body);
      expect(screen.queryByTestId('theme-menu')).not.toBeInTheDocument();
    });

    it('the toggle icon reflects the current mode', () => {
      const { rerender } = renderBar({ themeMode: 'light' });
      const buttonLight = screen.getByTestId('theme-toggle');
      expect(buttonLight).toHaveAttribute('aria-label', 'Theme: Light');

      rerender(
        <SearchBar
          ref={createRef()}
          value=""
          onChange={vi.fn()}
          onClear={vi.fn()}
          themeMode="system"
          onSetThemeMode={vi.fn()}
          onMouseDown={vi.fn()}
          activeFilter="all"
          onFilterChange={vi.fn()}
          deviceOptions={DEFAULT_DEVICES}
          selectedSource={null}
          onSourceChange={vi.fn()}
          appOptions={DEFAULT_APPS}
          selectedApp={null}
          onAppChange={vi.fn()}
        />
      );
      expect(screen.getByTestId('theme-toggle')).toHaveAttribute('aria-label', 'Theme: System');
    });
  });
});
