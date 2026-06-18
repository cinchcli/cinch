import { forwardRef, useEffect, useRef, useState, useCallback, type CSSProperties, type ReactNode } from 'react';
import { C } from '../design';
import { IconSearch, IconX, IconSun, IconMoon, IconMonitor } from '../icons';
import { CLIP_FILTERS, type ClipFilter } from '../lib/clipFilters';
import type { SourceColorSlot } from '../lib/sourceColor';

const FILTER_HINTS: Record<ClipFilter, string> = {
  all:   'show everything',
  text:  'plain / json',
  image: 'screenshots',
  code:  'code blocks',
  url:   'links',
};

export interface DeviceOption {
  source: string;
  label: string;
  count: number;
  colorSlot?: SourceColorSlot;
}

/// One entry in the source-app picker (the `>` filter): the macOS bundle id
/// (`id`, the stable filter key), the human display name (`label`), and how
/// many clips were captured from that app.
export interface AppOption {
  id: string;
  label: string;
  count: number;
}

type ThemeMode = 'light' | 'dark' | 'system';

interface SearchBarProps {
  value: string;
  onChange: (next: string) => void;
  onClear: () => void;
  themeMode: ThemeMode;
  onSetThemeMode: (mode: ThemeMode) => void;
  onMouseDown: (e: React.MouseEvent) => void;
  activeFilter: ClipFilter;
  onFilterChange: (f: ClipFilter) => void;
  deviceOptions: DeviceOption[];
  selectedSource: string | null;
  onSourceChange: (source: string | null) => void;
  appOptions: AppOption[];
  selectedApp: string | null;
  onAppChange: (app: string | null) => void;
}

const THEME_MODES: ThemeMode[] = ['light', 'dark', 'system'];

const THEME_LABEL: Record<ThemeMode, string> = {
  light:  'Light',
  dark:   'Dark',
  system: 'System',
};

const THEME_ICON: Record<ThemeMode, (size: number) => ReactNode> = {
  light:  (s) => <IconSun size={s} />,
  dark:   (s) => <IconMoon size={s} />,
  system: (s) => <IconMonitor size={s} />,
};

export const SearchBar = forwardRef<HTMLInputElement, SearchBarProps>(
  ({
    value, onChange, onClear, themeMode, onSetThemeMode, onMouseDown,
    activeFilter, onFilterChange,
    deviceOptions, selectedSource, onSourceChange,
    appOptions, selectedApp, onAppChange,
  }, ref) => {
    const [themeMenuOpen, setThemeMenuOpen] = useState(false);
    const themeMenuRef = useRef<HTMLDivElement | null>(null);

    useEffect(() => {
      if (!themeMenuOpen) return;
      const onPointer = (e: MouseEvent) => {
        if (themeMenuRef.current && !themeMenuRef.current.contains(e.target as Node)) {
          setThemeMenuOpen(false);
        }
      };
      const onKey = (e: KeyboardEvent) => {
        if (e.key === 'Escape') setThemeMenuOpen(false);
      };
      document.addEventListener('mousedown', onPointer);
      document.addEventListener('keydown', onKey);
      return () => {
        document.removeEventListener('mousedown', onPointer);
        document.removeEventListener('keydown', onKey);
      };
    }, [themeMenuOpen]);

    const [dropdownOpen, setDropdownOpen] = useState(false);
    const [dropdownQuery, setDropdownQuery] = useState('');
    const [highlightedFilter, setHighlightedFilter] = useState<ClipFilter>('all');

    const [deviceDropdownOpen, setDeviceDropdownOpen] = useState(false);
    const [deviceDropdownQuery, setDeviceDropdownQuery] = useState('');
    const [highlightedDevice, setHighlightedDevice] = useState<string | null>(null);

    const [appDropdownOpen, setAppDropdownOpen] = useState(false);
    const [appDropdownQuery, setAppDropdownQuery] = useState('');
    const [highlightedApp, setHighlightedApp] = useState<string | null>(null);

    const selectedDevice = deviceOptions.find((d) => d.source === selectedSource) ?? null;
    const selectedAppOption = appOptions.find((a) => a.id === selectedApp) ?? null;

    const matchingFilters = CLIP_FILTERS.filter(
      f => dropdownQuery === '' || f.startsWith(dropdownQuery)
    );

    const matchingDevices = deviceOptions.filter(
      d => deviceDropdownQuery === '' || d.label.toLowerCase().startsWith(deviceDropdownQuery)
    );

    const matchingApps = appOptions.filter(
      a => appDropdownQuery === '' || a.label.toLowerCase().startsWith(appDropdownQuery)
    );

    const openDropdown = useCallback((preHighlight: ClipFilter = 'all') => {
      setDeviceDropdownOpen(false);
      setDeviceDropdownQuery('');
      setAppDropdownOpen(false);
      setAppDropdownQuery('');
      setDropdownOpen(true);
      setDropdownQuery('');
      setHighlightedFilter(preHighlight);
    }, []);

    const closeDropdown = useCallback(() => {
      setDropdownOpen(false);
      setDropdownQuery('');
    }, []);

    const openDeviceDropdown = useCallback((preHighlight: string | null) => {
      setDropdownOpen(false);
      setDropdownQuery('');
      setAppDropdownOpen(false);
      setAppDropdownQuery('');
      setDeviceDropdownOpen(true);
      setDeviceDropdownQuery('');
      setHighlightedDevice(preHighlight ?? deviceOptions[0]?.source ?? null);
    }, [deviceOptions]);

    const closeDeviceDropdown = useCallback(() => {
      setDeviceDropdownOpen(false);
      setDeviceDropdownQuery('');
    }, []);

    const openAppDropdown = useCallback((preHighlight: string | null) => {
      setDropdownOpen(false);
      setDropdownQuery('');
      setDeviceDropdownOpen(false);
      setDeviceDropdownQuery('');
      setAppDropdownOpen(true);
      setAppDropdownQuery('');
      setHighlightedApp(preHighlight ?? appOptions[0]?.id ?? null);
    }, [appOptions]);

    const closeAppDropdown = useCallback(() => {
      setAppDropdownOpen(false);
      setAppDropdownQuery('');
    }, []);

    const selectFilter = useCallback((f: ClipFilter) => {
      onFilterChange(f);
      closeDropdown();
    }, [onFilterChange, closeDropdown]);

    const selectDevice = useCallback((source: string | null) => {
      onSourceChange(source);
      closeDeviceDropdown();
    }, [onSourceChange, closeDeviceDropdown]);

    const selectApp = useCallback((app: string | null) => {
      onAppChange(app);
      closeAppDropdown();
    }, [onAppChange, closeAppDropdown]);

    const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
      const raw = e.target.value;
      // `#` → type, `@` → device, `>` → app. Whichever sigil appears first in
      // the typed value wins; everything from it onward is stripped and the
      // matching dropdown opens (mirrors the original # behavior).
      const sigils: Array<[string, () => void]> = [
        ['#', () => openDropdown(CLIP_FILTERS[0])],
        ['@', () => openDeviceDropdown(deviceOptions[0]?.source ?? null)],
        ['>', () => openAppDropdown(appOptions[0]?.id ?? null)],
      ];
      let firstIdx = -1;
      let firstOpen: (() => void) | null = null;
      for (const [ch, open] of sigils) {
        const idx = raw.indexOf(ch);
        if (idx !== -1 && (firstIdx === -1 || idx < firstIdx)) {
          firstIdx = idx;
          firstOpen = open;
        }
      }
      if (firstOpen) {
        firstOpen();
        onChange(raw.slice(0, firstIdx));
        return;
      }
      onChange(raw);
    };

    // Keys consumed by an open dropdown must be hidden from the window-level
    // keydown listener in App.tsx (Enter copies the selected clip and hides
    // the window, ArrowUp/Down moves clip selection). React's preventDefault
    // doesn't block native bubbling, so we stop the native event explicitly.
    const consume = (e: React.KeyboardEvent) => {
      e.preventDefault();
      e.nativeEvent.stopImmediatePropagation();
    };

    const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
      // Backspace on empty input removes the closest chip, right-to-left in
      // render order: type, then app, then device.
      if (
        e.key === 'Backspace' &&
        !dropdownOpen &&
        !deviceDropdownOpen &&
        !appDropdownOpen &&
        value === ''
      ) {
        if (activeFilter !== 'all') {
          onFilterChange('all');
          return;
        }
        if (selectedApp !== null) {
          onAppChange(null);
          return;
        }
        if (selectedSource !== null) {
          onSourceChange(null);
          return;
        }
      }

      if (dropdownOpen) {
        handleFilterDropdownKey(e);
        return;
      }
      if (deviceDropdownOpen) {
        handleDeviceDropdownKey(e);
        return;
      }
      if (appDropdownOpen) {
        handleAppDropdownKey(e);
        return;
      }
    };

    const handleFilterDropdownKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
      const idx = matchingFilters.indexOf(highlightedFilter);
      const safeIdx = idx === -1 ? 0 : idx;

      if (e.key === 'ArrowDown') {
        consume(e);
        if (matchingFilters.length === 0) return;
        setHighlightedFilter(matchingFilters[(safeIdx + 1) % matchingFilters.length]);
        return;
      }
      if (e.key === 'ArrowUp') {
        consume(e);
        if (matchingFilters.length === 0) return;
        setHighlightedFilter(matchingFilters[(safeIdx - 1 + matchingFilters.length) % matchingFilters.length]);
        return;
      }
      if (e.key === 'Enter') {
        consume(e);
        selectFilter(highlightedFilter);
        return;
      }
      if (e.key === 'Escape') {
        consume(e);
        closeDropdown();
        return;
      }
      if (e.key === 'Backspace') {
        consume(e);
        if (dropdownQuery.length > 0) {
          const q = dropdownQuery.slice(0, -1);
          setDropdownQuery(q);
          const first = CLIP_FILTERS.find(f => q === '' || f.startsWith(q)) ?? highlightedFilter;
          setHighlightedFilter(first);
        } else {
          closeDropdown();
        }
        return;
      }
      if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
        consume(e);
        const q = dropdownQuery + e.key.toLowerCase();
        setDropdownQuery(q);
        const first = CLIP_FILTERS.find(f => f.startsWith(q)) ?? highlightedFilter;
        setHighlightedFilter(first);
      }
    };

    const handleDeviceDropdownKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
      const currentIdx = matchingDevices.findIndex((d) => d.source === highlightedDevice);
      const safeIdx = currentIdx === -1 ? 0 : currentIdx;

      if (e.key === 'ArrowDown') {
        consume(e);
        if (matchingDevices.length === 0) return;
        setHighlightedDevice(matchingDevices[(safeIdx + 1) % matchingDevices.length].source);
        return;
      }
      if (e.key === 'ArrowUp') {
        consume(e);
        if (matchingDevices.length === 0) return;
        setHighlightedDevice(matchingDevices[(safeIdx - 1 + matchingDevices.length) % matchingDevices.length].source);
        return;
      }
      if (e.key === 'Enter') {
        consume(e);
        if (highlightedDevice) selectDevice(highlightedDevice);
        return;
      }
      if (e.key === 'Escape') {
        consume(e);
        closeDeviceDropdown();
        return;
      }
      if (e.key === 'Backspace') {
        consume(e);
        if (deviceDropdownQuery.length > 0) {
          const q = deviceDropdownQuery.slice(0, -1);
          setDeviceDropdownQuery(q);
          const first = deviceOptions.find(d => q === '' || d.label.toLowerCase().startsWith(q));
          if (first) setHighlightedDevice(first.source);
        } else {
          closeDeviceDropdown();
        }
        return;
      }
      if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
        consume(e);
        const q = deviceDropdownQuery + e.key.toLowerCase();
        setDeviceDropdownQuery(q);
        const first = deviceOptions.find(d => d.label.toLowerCase().startsWith(q));
        if (first) setHighlightedDevice(first.source);
      }
    };

    const handleAppDropdownKey = (e: React.KeyboardEvent<HTMLInputElement>) => {
      const currentIdx = matchingApps.findIndex((a) => a.id === highlightedApp);
      const safeIdx = currentIdx === -1 ? 0 : currentIdx;

      if (e.key === 'ArrowDown') {
        consume(e);
        if (matchingApps.length === 0) return;
        setHighlightedApp(matchingApps[(safeIdx + 1) % matchingApps.length].id);
        return;
      }
      if (e.key === 'ArrowUp') {
        consume(e);
        if (matchingApps.length === 0) return;
        setHighlightedApp(matchingApps[(safeIdx - 1 + matchingApps.length) % matchingApps.length].id);
        return;
      }
      if (e.key === 'Enter') {
        consume(e);
        if (highlightedApp) selectApp(highlightedApp);
        return;
      }
      if (e.key === 'Escape') {
        consume(e);
        closeAppDropdown();
        return;
      }
      if (e.key === 'Backspace') {
        consume(e);
        if (appDropdownQuery.length > 0) {
          const q = appDropdownQuery.slice(0, -1);
          setAppDropdownQuery(q);
          const first = appOptions.find(a => q === '' || a.label.toLowerCase().startsWith(q));
          if (first) setHighlightedApp(first.id);
        } else {
          closeAppDropdown();
        }
        return;
      }
      if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
        consume(e);
        const q = appDropdownQuery + e.key.toLowerCase();
        setAppDropdownQuery(q);
        const first = appOptions.find(a => a.label.toLowerCase().startsWith(q));
        if (first) setHighlightedApp(first.id);
      }
    };

    const placeholder =
      activeFilter !== 'all' || selectedSource !== null || selectedApp !== null
        ? ''
        : 'Search clips…  # type, @ device, > app';

    return (
      <div style={S.bar} onMouseDown={onMouseDown} data-testid="search-bar">
        <span style={S.glass}><IconSearch size={14} /></span>

        {selectedDevice && (
            <span
              style={{ ...S.chip, background: 'var(--accent-subtle)', color: 'var(--accent)', border: '1px solid transparent' }}
              data-testid="device-chip"
              onClick={() => openDeviceDropdown(selectedDevice.source)}
            >
              <span style={{ ...S.chipDot, background: 'var(--accent)' }} />
              {selectedDevice.label}
              <span
                style={S.chipX}
                data-testid="device-chip-x"
                onClick={(e) => { e.stopPropagation(); onSourceChange(null); }}
              >
                ✕
              </span>
            </span>
        )}

        {selectedAppOption && (
            <span
              style={{ ...S.chip, background: 'var(--accent-subtle)', color: 'var(--accent)', border: '1px solid transparent' }}
              data-testid="app-chip"
              onClick={() => openAppDropdown(selectedAppOption.id)}
            >
              <img
                src={`cinch://app-icon/${encodeURIComponent(selectedAppOption.id)}`}
                alt=""
                aria-hidden="true"
                style={S.chipIcon}
                onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none'; }}
              />
              {selectedAppOption.label}
              <span
                style={S.chipX}
                data-testid="app-chip-x"
                onClick={(e) => { e.stopPropagation(); onAppChange(null); }}
              >
                ✕
              </span>
            </span>
        )}

        {activeFilter !== 'all' && (
          <span
            style={{ ...S.chip, ...S[`chip_${activeFilter}`] }}
            data-testid="filter-chip"
            onClick={() => openDropdown(activeFilter)}
          >
            <span style={{ ...S.chipDot, ...S[`dot_${activeFilter}`] }} />
            {activeFilter}
            <span
              style={S.chipX}
              data-testid="filter-chip-x"
              onClick={(e) => { e.stopPropagation(); onFilterChange('all'); }}
            >
              ✕
            </span>
          </span>
        )}

        <input
          ref={ref}
          type="text"
          value={value}
          onChange={handleInputChange}
          onKeyDown={handleKeyDown}
          placeholder={placeholder}
          aria-label="Search clips"
          spellCheck={false}
          autoFocus
          style={S.input}
        />

        {value && (
          <button type="button" onClick={onClear} aria-label="Clear search" className="icon-btn" style={S.iconBtn}>
            <IconX size={12} />
          </button>
        )}
        <div ref={themeMenuRef} style={S.themeAnchor}>
          <button
            type="button"
            onClick={() => setThemeMenuOpen((v) => !v)}
            aria-label={`Theme: ${THEME_LABEL[themeMode]}`}
            aria-haspopup="menu"
            aria-expanded={themeMenuOpen}
            title={`Theme: ${THEME_LABEL[themeMode]}`}
            className="icon-btn"
            style={S.iconBtn}
            data-testid="theme-toggle"
          >
            {THEME_ICON[themeMode](14)}
          </button>
          {themeMenuOpen && (
            <div style={S.themeMenu} role="menu" data-testid="theme-menu">
              {THEME_MODES.map((m) => {
                const active = m === themeMode;
                return (
                  <div
                    key={m}
                    role="menuitemradio"
                    aria-checked={active}
                    data-testid={`theme-option-${m}`}
                    style={{ ...S.themeItem, ...(active ? S.themeItemHL : {}) }}
                    onMouseDown={(e) => {
                      e.preventDefault();
                      onSetThemeMode(m);
                      setThemeMenuOpen(false);
                    }}
                  >
                    <span style={S.themeIcon}>{THEME_ICON[m](13)}</span>
                    <span style={S.themeLabel}>{THEME_LABEL[m]}</span>
                    {active && <span style={S.themeCheck} aria-hidden="true">✓</span>}
                  </div>
                );
              })}
            </div>
          )}
        </div>

        {dropdownOpen && (
          <div style={S.dropdown} data-testid="filter-dropdown">
            {CLIP_FILTERS.map((f) => {
              const matches = dropdownQuery === '' || f.startsWith(dropdownQuery);
              return (
                <div
                  key={f}
                  style={{
                    ...S.dropItem,
                    ...(highlightedFilter === f ? S.dropItemHL : {}),
                    ...(!matches ? S.dropItemDim : {}),
                  }}
                  aria-selected={highlightedFilter === f}
                  data-testid={`filter-option-${f}`}
                  onMouseDown={(e) => { e.preventDefault(); selectFilter(f); }}
                >
                  <span style={{ ...S.dropDot, ...S[`dot_${f}`] }} />
                  {f}
                  <span style={S.dropHint}>{FILTER_HINTS[f]}</span>
                </div>
              );
            })}
          </div>
        )}

        {deviceDropdownOpen && (
          <div style={S.dropdown} data-testid="device-dropdown">
            {/* No "all devices" entry — the chip's ✕ and Backspace-on-empty
                already handle "clear filter", and listing it inside the
                dropdown when no chip is set is just noise. */}
            {deviceOptions.map((d) => {
              const matches = deviceDropdownQuery === '' || d.label.toLowerCase().startsWith(deviceDropdownQuery);
              return (
                <div
                  key={d.source}
                  style={{
                    ...S.dropItem,
                    ...(highlightedDevice === d.source ? S.dropItemHL : {}),
                    ...(!matches ? S.dropItemDim : {}),
                  }}
                  aria-selected={highlightedDevice === d.source}
                  data-testid={`device-option-${d.source}`}
                  onMouseDown={(e) => { e.preventDefault(); selectDevice(d.source); }}
                >
                  <span style={{ ...S.dropDot, background: C.t3 }} />
                  {d.label}
                  <span style={S.dropHint}>{d.count} clip{d.count === 1 ? '' : 's'}</span>
                </div>
              );
            })}
            {deviceOptions.length === 0 && (
              <div style={{ ...S.dropItem, opacity: 0.55 }} data-testid="device-option-empty">
                no devices yet
              </div>
            )}
          </div>
        )}

        {appDropdownOpen && (
          <div style={S.dropdown} data-testid="app-dropdown">
            {appOptions.map((a) => {
              const matches = appDropdownQuery === '' || a.label.toLowerCase().startsWith(appDropdownQuery);
              return (
                <div
                  key={a.id}
                  style={{
                    ...S.dropItem,
                    ...(highlightedApp === a.id ? S.dropItemHL : {}),
                    ...(!matches ? S.dropItemDim : {}),
                  }}
                  aria-selected={highlightedApp === a.id}
                  data-testid={`app-option-${a.id}`}
                  onMouseDown={(e) => { e.preventDefault(); selectApp(a.id); }}
                >
                  <img
                    src={`cinch://app-icon/${encodeURIComponent(a.id)}`}
                    alt=""
                    aria-hidden="true"
                    style={S.appIcon}
                    onError={(e) => { (e.currentTarget as HTMLImageElement).style.display = 'none'; }}
                  />
                  {a.label}
                  <span style={S.dropHint}>{a.count} clip{a.count === 1 ? '' : 's'}</span>
                </div>
              );
            })}
            {appOptions.length === 0 && (
              <div style={{ ...S.dropItem, opacity: 0.55 }} data-testid="app-option-empty">
                no apps yet
              </div>
            )}
          </div>
        )}
      </div>
    );
  }
);

SearchBar.displayName = 'SearchBar';

const S: Record<string, CSSProperties> = {
  bar: {
    display: 'flex',
    alignItems: 'center',
    height: 50,
    padding: '0 18px',
    gap: 12,
    background: C.card,
    borderBottom: `1px solid ${C.border}`,
    flexShrink: 0,
    position: 'relative',
  },
  glass: { color: C.t2, display: 'flex', alignItems: 'center' },
  input: {
    flex: 1,
    background: 'transparent',
    border: 'none',
    outline: 'none',
    fontFamily: 'var(--font-body)',
    fontSize: 15,
    fontWeight: 400,
    letterSpacing: '-0.01em',
    color: C.t1,
    minWidth: 0,
  },
  iconBtn: {
    background: 'transparent',
    border: 'none',
    color: C.t3,
    padding: 4,
    display: 'flex',
    alignItems: 'center',
    cursor: 'pointer',
    borderRadius: 4,
  },
  chip: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 4,
    padding: '2px 7px',
    borderRadius: 20,
    fontSize: 10,
    fontFamily: 'var(--font-mono)',
    letterSpacing: '0.03em',
    flexShrink: 0,
    cursor: 'pointer',
    userSelect: 'none',
  },
  chip_text:  { background: 'var(--accent-subtle)', color: 'var(--accent)', border: '1px solid transparent' },
  chip_image: { background: 'var(--accent-subtle)', color: 'var(--accent)', border: '1px solid transparent' },
  chip_code:  { background: 'var(--accent-subtle)', color: 'var(--accent)', border: '1px solid transparent' },
  chip_url:   { background: 'var(--accent-subtle)', color: 'var(--accent)', border: '1px solid transparent' },
  chipDot: {
    width: 5,
    height: 5,
    borderRadius: '50%',
    background: 'currentColor',
    flexShrink: 0,
  },
  chipIcon: {
    width: 12,
    height: 12,
    borderRadius: 2,
    flexShrink: 0,
    objectFit: 'contain',
  },
  appIcon: {
    width: 14,
    height: 14,
    borderRadius: 3,
    flexShrink: 0,
    objectFit: 'contain',
  },
  chipX: {
    fontSize: 9,
    opacity: 0.55,
    marginLeft: 2,
    cursor: 'pointer',
  },
  dropdown: {
    position: 'absolute',
    top: '100%',
    left: 0,
    right: 0,
    // `C.card2` (var(--surface-2)) is a 5%-alpha elevation overlay in dark
    // mode — fine for inline surfaces like the Rail, but invisible when the
    // dropdown floats over the ClipList. `C.card` (var(--surface)) is opaque
    // in both themes, and the box-shadow gives the needed elevation cue.
    background: C.card,
    border: `1px solid ${C.border}`,
    borderTop: 'none',
    boxShadow: '0 6px 16px rgba(0, 0, 0, 0.22), 0 1px 3px rgba(0, 0, 0, 0.10)',
    zIndex: 100,
    padding: '3px 0',
  },
  dropItem: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    padding: '6px 18px',
    fontSize: 12,
    fontFamily: 'var(--font-mono)',
    cursor: 'pointer',
    color: C.t2,
  },
  dropItemHL: {
    background: C.selected,
    color: C.t1,
  },
  dropItemDim: {
    opacity: 0.28,
  },
  dropHint: {
    marginLeft: 'auto',
    fontSize: 11,
    color: C.t4,
  },
  dropDot: {
    width: 5,
    height: 5,
    borderRadius: '50%',
    flexShrink: 0,
  },
  dot_all:   { background: C.t3 },
  dot_text:  { background: C.accent },
  dot_image: { background: C.accent },
  dot_code:  { background: C.accent },
  dot_url:   { background: C.accent },
  themeAnchor: {
    position: 'relative',
    display: 'flex',
    alignItems: 'center',
  },
  themeMenu: {
    position: 'absolute',
    top: 'calc(100% + 6px)',
    right: 0,
    width: 180,
    background: C.card2,
    border: `1px solid ${C.border}`,
    borderRadius: 6,
    padding: '4px 0',
    zIndex: 110,
    boxShadow: '0 8px 20px rgba(0,0,0,0.28)',
  },
  themeItem: {
    display: 'flex',
    alignItems: 'center',
    gap: 10,
    padding: '7px 12px',
    fontSize: 12,
    fontFamily: 'var(--font-mono)',
    cursor: 'pointer',
    color: C.t2,
    userSelect: 'none',
  },
  themeItemHL: {
    color: C.t1,
  },
  themeIcon: {
    display: 'inline-flex',
    width: 16,
    justifyContent: 'center',
    color: C.t2,
  },
  themeLabel: {
    flex: 1,
  },
  themeCheck: {
    fontSize: 12,
    color: C.t1,
    marginLeft: 'auto',
  },
};
