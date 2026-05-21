import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { createRef } from 'react';
import { SearchBar, type DeviceOption } from './SearchBar';
import type { ClipFilter } from '../lib/clipFilters';

const DEFAULT_DEVICES: DeviceOption[] = [
  { source: 'remote:macbook', label: 'MacBook',    count: 87, colorSlot: 'mint' },
  { source: 'remote:iphone',  label: 'iPhone',     count: 31, colorSlot: 'sky' },
  { source: 'remote:linux',   label: 'Linux Box',  count: 24, colorSlot: 'amber' },
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
  themeMode: ThemeMode;
  onSetThemeMode: (m: ThemeMode) => void;
}> = {}) {
  const onChange = overrides.onChange ?? vi.fn();
  const onFilterChange = overrides.onFilterChange ?? vi.fn();
  const onSourceChange = overrides.onSourceChange ?? vi.fn();
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
    />
  );
  return { ...result, onChange, onFilterChange, onSourceChange, onSetThemeMode };
}

describe('SearchBar', () => {
  it('renders search input', () => {
    renderBar();
    expect(screen.getByPlaceholderText(/search clips/i)).toBeInTheDocument();
  });

  describe('filter dropdown', () => {
    it('opens when # is typed in the input', () => {
      renderBar();
      const input = screen.getByPlaceholderText(/search clips/i);
      fireEvent.change(input, { target: { value: '#' } });
      expect(screen.getByTestId('filter-dropdown')).toBeInTheDocument();
    });

    it('shows all five filter options when dropdown is open', () => {
      renderBar();
      fireEvent.change(screen.getByPlaceholderText(/search clips/i), { target: { value: '#' } });
      expect(screen.getByTestId('filter-option-all')).toBeInTheDocument();
      expect(screen.getByTestId('filter-option-text')).toBeInTheDocument();
      expect(screen.getByTestId('filter-option-image')).toBeInTheDocument();
      expect(screen.getByTestId('filter-option-code')).toBeInTheDocument();
      expect(screen.getByTestId('filter-option-url')).toBeInTheDocument();
    });

    it('strips # from the value passed to onChange', () => {
      const { onChange } = renderBar({ value: 'hello' });
      fireEvent.change(screen.getByPlaceholderText(/search clips/i), { target: { value: 'hello#' } });
      expect(onChange).toHaveBeenCalledWith('hello');
    });

    it('closes on Escape without calling onFilterChange', () => {
      const { onFilterChange } = renderBar();
      const input = screen.getByPlaceholderText(/search clips/i);
      fireEvent.change(input, { target: { value: '#' } });
      fireEvent.keyDown(input, { key: 'Escape' });
      expect(screen.queryByTestId('filter-dropdown')).not.toBeInTheDocument();
      expect(onFilterChange).not.toHaveBeenCalled();
    });

    it('dims non-matching items when a letter is typed after #', () => {
      renderBar();
      const input = screen.getByPlaceholderText(/search clips/i);
      fireEvent.change(input, { target: { value: '#' } });
      fireEvent.keyDown(input, { key: 'c' });
      // 'code' matches 'c', others do not
      const codeOption = screen.getByTestId('filter-option-code');
      const textOption = screen.getByTestId('filter-option-text');
      expect(codeOption).not.toHaveStyle({ opacity: '0.28' });
      expect(textOption).toHaveStyle({ opacity: '0.28' });
    });

    it('Enter selects the highlighted filter and closes dropdown', () => {
      const { onFilterChange } = renderBar();
      const input = screen.getByPlaceholderText(/search clips/i);
      fireEvent.change(input, { target: { value: '#' } });
      fireEvent.keyDown(input, { key: 'c' });
      fireEvent.keyDown(input, { key: 'Enter' });
      expect(onFilterChange).toHaveBeenCalledWith('code');
      expect(screen.queryByTestId('filter-dropdown')).not.toBeInTheDocument();
    });

    it('ArrowDown moves highlight to the next matching item', () => {
      renderBar();
      const input = screen.getByPlaceholderText(/search clips/i);
      fireEvent.change(input, { target: { value: '#' } });
      // initial highlight is 'all' (CLIP_FILTERS[0]); ArrowDown → 'text'
      fireEvent.keyDown(input, { key: 'ArrowDown' });
      expect(screen.getByTestId('filter-option-text')).toHaveAttribute('aria-selected', 'true');
      expect(screen.getByTestId('filter-option-all')).toHaveAttribute('aria-selected', 'false');
    });

    it('clicking a dropdown item selects it', () => {
      const { onFilterChange } = renderBar();
      const input = screen.getByPlaceholderText(/search clips/i);
      fireEvent.change(input, { target: { value: '#' } });
      fireEvent.mouseDown(screen.getByTestId('filter-option-image'));
      expect(onFilterChange).toHaveBeenCalledWith('image');
    });

    it('selecting "all" calls onFilterChange with "all"', () => {
      const { onFilterChange } = renderBar({ activeFilter: 'image' });
      const input = screen.getByLabelText('Search clips');
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
      // 'code' option should have the highlighted style applied
      const codeOption = screen.getByTestId('filter-option-code');
      // highlighted means it does not have the dim style
      expect(codeOption).not.toHaveStyle({ opacity: '0.28' });
    });

    it('Backspace on empty input with active filter calls onFilterChange("all")', () => {
      const { onFilterChange } = renderBar({ value: '', activeFilter: 'url' });
      const input = screen.getByLabelText('Search clips');
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
      renderBar();
      const input = screen.getByLabelText('Search clips');
      fireEvent.change(input, { target: { value: '@' } });
      expect(screen.getByTestId('device-dropdown')).toBeInTheDocument();
    });

    it('lists every device with no "all devices" entry', () => {
      renderBar();
      fireEvent.change(screen.getByLabelText('Search clips'), { target: { value: '@' } });
      expect(screen.queryByTestId('device-option-all')).not.toBeInTheDocument();
      expect(screen.getByTestId('device-option-remote:macbook')).toBeInTheDocument();
      expect(screen.getByTestId('device-option-remote:iphone')).toBeInTheDocument();
      expect(screen.getByTestId('device-option-remote:linux')).toBeInTheDocument();
    });

    it('strips @ from the value passed to onChange', () => {
      const { onChange } = renderBar({ value: 'hello' });
      fireEvent.change(screen.getByLabelText('Search clips'), { target: { value: 'hello@' } });
      expect(onChange).toHaveBeenCalledWith('hello');
    });

    it('autocomplete narrows by label prefix', () => {
      renderBar();
      const input = screen.getByLabelText('Search clips');
      fireEvent.change(input, { target: { value: '@' } });
      fireEvent.keyDown(input, { key: 'i' });
      const iphone = screen.getByTestId('device-option-remote:iphone');
      const macbook = screen.getByTestId('device-option-remote:macbook');
      expect(iphone).not.toHaveStyle({ opacity: '0.28' });
      expect(macbook).toHaveStyle({ opacity: '0.28' });
    });

    it('Enter selects highlighted device and closes dropdown', () => {
      const { onSourceChange } = renderBar();
      const input = screen.getByLabelText('Search clips');
      fireEvent.change(input, { target: { value: '@' } });
      fireEvent.keyDown(input, { key: 'i' });
      fireEvent.keyDown(input, { key: 'Enter' });
      expect(onSourceChange).toHaveBeenCalledWith('remote:iphone');
      expect(screen.queryByTestId('device-dropdown')).not.toBeInTheDocument();
    });

    it('ArrowDown moves highlight to the next device', () => {
      renderBar();
      const input = screen.getByLabelText('Search clips');
      fireEvent.change(input, { target: { value: '@' } });
      // initial highlight is the first device (MacBook); ArrowDown → iPhone
      fireEvent.keyDown(input, { key: 'ArrowDown' });
      expect(screen.getByTestId('device-option-remote:iphone')).toHaveAttribute('aria-selected', 'true');
      expect(screen.getByTestId('device-option-remote:macbook')).toHaveAttribute('aria-selected', 'false');
    });

    it('clicking a device option selects it', () => {
      const { onSourceChange } = renderBar();
      fireEvent.change(screen.getByLabelText('Search clips'), { target: { value: '@' } });
      fireEvent.mouseDown(screen.getByTestId('device-option-remote:linux'));
      expect(onSourceChange).toHaveBeenCalledWith('remote:linux');
    });

    // Clearing the chip is handled by ✕ / Backspace-on-empty / direct call
    // to onSourceChange(null) — covered in the "device chip" suite. The
    // dropdown itself no longer carries a "clear" entry.

    it('Escape closes the device dropdown without calling onSourceChange', () => {
      const { onSourceChange } = renderBar();
      const input = screen.getByLabelText('Search clips');
      fireEvent.change(input, { target: { value: '@' } });
      fireEvent.keyDown(input, { key: 'Escape' });
      expect(screen.queryByTestId('device-dropdown')).not.toBeInTheDocument();
      expect(onSourceChange).not.toHaveBeenCalled();
    });

    it('opening @ closes the # dropdown (mutually exclusive)', () => {
      renderBar();
      const input = screen.getByLabelText('Search clips');
      fireEvent.change(input, { target: { value: '#' } });
      expect(screen.getByTestId('filter-dropdown')).toBeInTheDocument();
      fireEvent.change(input, { target: { value: '@' } });
      expect(screen.queryByTestId('filter-dropdown')).not.toBeInTheDocument();
      expect(screen.getByTestId('device-dropdown')).toBeInTheDocument();
    });

    it('shows empty-state when there are no devices', () => {
      renderBar({ deviceOptions: [] });
      fireEvent.change(screen.getByLabelText('Search clips'), { target: { value: '@' } });
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
      const { onSourceChange } = renderBar({ value: '', selectedSource: 'remote:linux' });
      const input = screen.getByLabelText('Search clips');
      fireEvent.keyDown(input, { key: 'Backspace' });
      expect(onSourceChange).toHaveBeenCalledWith(null);
    });

    it('Backspace on empty input clears type chip first when both are set', () => {
      const { onSourceChange, onFilterChange } = renderBar({
        value: '',
        activeFilter: 'code',
        selectedSource: 'remote:macbook',
      });
      const input = screen.getByLabelText('Search clips');
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
        renderBar();
        const input = screen.getByLabelText('Search clips');
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
        renderBar();
        const input = screen.getByLabelText('Search clips');
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
        renderBar();
        const input = screen.getByLabelText('Search clips');
        fireEvent.change(input, { target: { value: '@' } });
        fireEvent.keyDown(input, { key: 'ArrowDown' });
        fireEvent.keyDown(input, { key: 'ArrowUp' });
        expect(tracker.seen).toEqual([]);
      } finally {
        tracker.cleanup();
      }
    });

    it('Enter outside any dropdown still reaches window listeners', () => {
      // Sanity check: the isolation is scoped to open dropdowns. With both
      // dropdowns closed, Enter should bubble normally — App.tsx relies on
      // this path to copy the selected clip when the user hits Enter in the
      // search field.
      const tracker = trackWindowKeydowns(new Set(['Enter']));
      try {
        renderBar();
        const input = screen.getByLabelText('Search clips');
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
        />
      );
      expect(screen.getByTestId('theme-toggle')).toHaveAttribute('aria-label', 'Theme: System');
    });
  });
});
