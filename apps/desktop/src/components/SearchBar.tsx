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

// The three sigil-triggered filters share one "mode" state machine. Typing a
// sigil enters a mode; the text after it is the live query (echoed in the
// input); Enter/click commits the highlighted row; Escape / Backspace-on-empty
// exits. Only one mode is ever active, so two dropdowns can never be open at
// once (this is structural, not enforced by hand).
type Mode = 'type' | 'device' | 'app';

// First sigil wins on entry; ordered so we can scan for the earliest one.
const SIGIL_MODE: ReadonlyArray<readonly [string, Mode]> = [
  ['#', 'type'],
  ['@', 'device'],
  ['>', 'app'],
];

const MODE_SIGIL: Record<Mode, string> = { type: '#', device: '@', app: '>' };
const MODE_PILL: Record<Mode, string> = { type: 'type', device: 'device', app: 'app' };
const MODE_PLACEHOLDER: Record<Mode, string> = {
  type:   'filter by type…',
  device: 'filter by device…',
  app:    'filter by app…',
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

    // The unified filter-mode state. `highlightId` holds whichever id type the
    // active mode uses (ClipFilter / device source / app bundle id) — all
    // strings, so one field covers all three.
    const [mode, setMode] = useState<Mode | null>(null);
    const [query, setQuery] = useState('');
    const [highlightId, setHighlightId] = useState<string | null>(null);

    const selectedDevice = deviceOptions.find((d) => d.source === selectedSource) ?? null;
    const selectedAppOption = appOptions.find((a) => a.id === selectedApp) ?? null;

    // The ordered list of option ids that match `q` in mode `m`, by label
    // prefix (case-insensitive). Used both to seed the highlight on entry /
    // query change and to drive the rendered dropdown.
    const matchingIdsFor = useCallback((m: Mode, q: string): string[] => {
      const lower = q.toLowerCase();
      if (m === 'type') return CLIP_FILTERS.filter((f) => lower === '' || f.startsWith(lower));
      if (m === 'device') {
        return deviceOptions
          .filter((d) => lower === '' || d.label.toLowerCase().startsWith(lower))
          .map((d) => d.source);
      }
      return appOptions
        .filter((a) => lower === '' || a.label.toLowerCase().startsWith(lower))
        .map((a) => a.id);
    }, [deviceOptions, appOptions]);

    const enterMode = useCallback((m: Mode, initialQuery: string, preHighlight?: string | null) => {
      setMode(m);
      setQuery(initialQuery);
      const ids = matchingIdsFor(m, initialQuery);
      setHighlightId(preHighlight ?? ids[0] ?? null);
    }, [matchingIdsFor]);

    const exitMode = useCallback(() => {
      setMode(null);
      setQuery('');
      setHighlightId(null);
    }, []);

    const commitId = useCallback((id: string) => {
      if (mode === 'type') onFilterChange(id as ClipFilter);
      else if (mode === 'device') onSourceChange(id);
      else if (mode === 'app') onAppChange(id);
    }, [mode, onFilterChange, onSourceChange, onAppChange]);

    // Rows for the currently-open dropdown (empty when no mode is active).
    const q = query.toLowerCase();
    const matchingFilters = mode === 'type'
      ? CLIP_FILTERS.filter((f) => q === '' || f.startsWith(q))
      : [];
    const matchingDevices = mode === 'device'
      ? deviceOptions.filter((d) => q === '' || d.label.toLowerCase().startsWith(q))
      : [];
    const matchingApps = mode === 'app'
      ? appOptions.filter((a) => q === '' || a.label.toLowerCase().startsWith(q))
      : [];
    const matchingIds: string[] =
      mode === 'type'   ? matchingFilters
      : mode === 'device' ? matchingDevices.map((d) => d.source)
      : mode === 'app'    ? matchingApps.map((a) => a.id)
      : [];

    // `highlightId` holds the user's sticky intent, but the options can change
    // under it (a clip arriving updates counts / re-sorts the list, or a device
    // drops off). Coalesce to a row that actually exists in the current list so
    // the highlight never points at a vanished row — it always "tracks the top
    // match". Used for rendering, keyboard nav, and commit.
    const effectiveHighlightId =
      highlightId !== null && matchingIds.includes(highlightId)
        ? highlightId
        : matchingIds[0] ?? null;

    const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
      const raw = e.target.value;
      // Inside a mode the whole input IS the query — sigils are literal text,
      // so you can't nest modes.
      if (mode !== null) {
        setQuery(raw);
        setHighlightId(matchingIdsFor(mode, raw)[0] ?? null);
        return;
      }
      // Not in a mode: whichever sigil appears first opens its mode. Text
      // before the sigil stays as the clip-search value; text after it becomes
      // the initial filter query.
      let firstIdx = -1;
      let firstMode: Mode | null = null;
      for (const [ch, m] of SIGIL_MODE) {
        const idx = raw.indexOf(ch);
        if (idx !== -1 && (firstIdx === -1 || idx < firstIdx)) {
          firstIdx = idx;
          firstMode = m;
        }
      }
      if (firstMode !== null) {
        enterMode(firstMode, raw.slice(firstIdx + 1));
        onChange(raw.slice(0, firstIdx));
        return;
      }
      onChange(raw);
    };

    // Keys handled by an open dropdown must be hidden from the window-level
    // keydown listener in App.tsx (Enter copies the selected clip and hides
    // the window, ArrowUp/Down moves clip selection). React's preventDefault
    // doesn't block native bubbling, so we stop the native event explicitly.
    const consume = (e: React.KeyboardEvent) => {
      e.preventDefault();
      e.nativeEvent.stopImmediatePropagation();
    };

    const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
      if (mode === null) {
        // Backspace on empty input removes the closest chip, right-to-left in
        // render order: type, then app, then device.
        if (e.key === 'Backspace' && value === '') {
          if (activeFilter !== 'all') { onFilterChange('all'); return; }
          if (selectedApp !== null) { onAppChange(null); return; }
          if (selectedSource !== null) { onSourceChange(null); return; }
        }
        // Everything else bubbles — App.tsx relies on Enter / Arrows here.
        return;
      }

      // In a filter mode: navigation keys are handled (and isolated) here;
      // character keys fall through to the input so the query stays visible.
      const curIdx = effectiveHighlightId === null ? -1 : matchingIds.indexOf(effectiveHighlightId);
      const safeIdx = curIdx === -1 ? 0 : curIdx;

      if (e.key === 'ArrowDown') {
        consume(e);
        if (matchingIds.length > 0) setHighlightId(matchingIds[(safeIdx + 1) % matchingIds.length]);
        return;
      }
      if (e.key === 'ArrowUp') {
        consume(e);
        if (matchingIds.length > 0) {
          setHighlightId(matchingIds[(safeIdx - 1 + matchingIds.length) % matchingIds.length]);
        }
        return;
      }
      if (e.key === 'Enter') {
        consume(e);
        if (effectiveHighlightId !== null) {
          commitId(effectiveHighlightId);
          exitMode();
        }
        return;
      }
      if (e.key === 'Escape') {
        consume(e);
        exitMode();
        return;
      }
      if (e.key === 'Backspace') {
        // Edit the query here (rather than letting the input mutate natively)
        // so the behavior is deterministic and Backspace never reaches the
        // window-level handler. An empty query exits the mode.
        consume(e);
        if (query === '') {
          exitMode();
        } else {
          const next = query.slice(0, -1);
          setQuery(next);
          setHighlightId(matchingIdsFor(mode, next)[0] ?? null);
        }
        return;
      }
      // Printable characters and the rest fall through to the input.
    };

    const placeholder =
      mode !== null
        ? MODE_PLACEHOLDER[mode]
        : activeFilter !== 'all' || selectedSource !== null || selectedApp !== null
          ? ''
          : 'Search clips…  # type, @ device, > app';

    return (
      <div style={S.bar} onMouseDown={onMouseDown} data-testid="search-bar">
        <span style={S.glass}><IconSearch size={14} /></span>

        {selectedDevice && (
            <span
              style={{ ...S.chip, background: 'var(--accent-subtle)', color: 'var(--accent)', border: '1px solid transparent' }}
              data-testid="device-chip"
              onClick={() => enterMode('device', '', selectedDevice.source)}
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
              onClick={() => enterMode('app', '', selectedAppOption.id)}
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
            onClick={() => enterMode('type', '', activeFilter)}
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

        {mode !== null && (
          <span style={S.modePill} data-testid="mode-pill">
            <span style={S.modePillSigil}>{MODE_SIGIL[mode]}</span>
            {MODE_PILL[mode]}
          </span>
        )}

        <input
          ref={ref}
          type="text"
          value={mode !== null ? query : value}
          onChange={handleInputChange}
          onKeyDown={handleKeyDown}
          placeholder={placeholder}
          aria-label="Search clips"
          spellCheck={false}
          autoFocus
          style={S.input}
        />

        {mode === null && value && (
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

        {mode === 'type' && (
          <div style={S.dropdown} role="listbox" data-testid="filter-dropdown">
            {matchingFilters.map((f) => (
              <div
                key={f}
                role="option"
                style={{ ...S.dropItem, ...(effectiveHighlightId === f ? S.dropItemHL : {}) }}
                aria-selected={effectiveHighlightId === f}
                data-testid={`filter-option-${f}`}
                onMouseDown={(e) => { e.preventDefault(); commitId(f); exitMode(); }}
              >
                <span style={{ ...S.dropDot, ...S[`dot_${f}`] }} />
                {f}
                <span style={S.dropHint}>{FILTER_HINTS[f]}</span>
              </div>
            ))}
            {matchingFilters.length === 0 && (
              <div style={{ ...S.dropItem, opacity: 0.55 }} role="status" data-testid="filter-option-empty">
                no matches
              </div>
            )}
          </div>
        )}

        {mode === 'device' && (
          <div style={S.dropdown} role="listbox" data-testid="device-dropdown">
            {/* No "all devices" entry — the chip's ✕ and Backspace-on-empty
                already handle "clear filter", and listing it inside the
                dropdown when no chip is set is just noise. */}
            {matchingDevices.map((d) => (
              <div
                key={d.source}
                role="option"
                style={{ ...S.dropItem, ...(effectiveHighlightId === d.source ? S.dropItemHL : {}) }}
                aria-selected={effectiveHighlightId === d.source}
                data-testid={`device-option-${d.source}`}
                onMouseDown={(e) => { e.preventDefault(); commitId(d.source); exitMode(); }}
              >
                <span style={{ ...S.dropDot, background: C.t3 }} />
                {d.label}
                <span style={S.dropHint}>{d.count} clip{d.count === 1 ? '' : 's'}</span>
              </div>
            ))}
            {matchingDevices.length === 0 && (
              <div style={{ ...S.dropItem, opacity: 0.55 }} role="status" data-testid="device-option-empty">
                {deviceOptions.length === 0 ? 'no devices yet' : 'no matches'}
              </div>
            )}
          </div>
        )}

        {mode === 'app' && (
          <div style={S.dropdown} role="listbox" data-testid="app-dropdown">
            {matchingApps.map((a) => (
              <div
                key={a.id}
                role="option"
                style={{ ...S.dropItem, ...(effectiveHighlightId === a.id ? S.dropItemHL : {}) }}
                aria-selected={effectiveHighlightId === a.id}
                data-testid={`app-option-${a.id}`}
                onMouseDown={(e) => { e.preventDefault(); commitId(a.id); exitMode(); }}
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
            ))}
            {matchingApps.length === 0 && (
              <div style={{ ...S.dropItem, opacity: 0.55 }} role="status" data-testid="app-option-empty">
                {appOptions.length === 0 ? 'no apps yet' : 'no matches'}
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
  // The mode prefix pill is transient (no ✕) and intentionally muted/bordered
  // so it reads differently from the accent-filled committed chips.
  modePill: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 4,
    padding: '2px 8px',
    borderRadius: 20,
    fontSize: 10,
    fontFamily: 'var(--font-mono)',
    letterSpacing: '0.03em',
    flexShrink: 0,
    background: C.card2,
    color: C.t2,
    border: `1px solid ${C.border}`,
    userSelect: 'none',
  },
  modePillSigil: {
    color: C.accent,
    fontWeight: 600,
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
