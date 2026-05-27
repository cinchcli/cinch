import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import { dialogStyles } from './dialogPrimitives';
import { C } from '../design';
import { IconSearch, IconX } from '../icons';

export interface TransformAction {
  id: string;
  label: string;
}

interface TransformCopySheetProps {
  actions: TransformAction[];
  onSelect: (actionId: string) => void;
  onClose: () => void;
}

export function TransformCopySheet({ actions, onSelect, onClose }: TransformCopySheetProps) {
  const [query, setQuery] = useState('');
  const [highlight, setHighlight] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return actions;
    return actions.filter((action) => {
      return action.label.toLowerCase().includes(q) || action.id.toLowerCase().includes(q);
    });
  }, [actions, query]);

  useEffect(() => {
    setHighlight(0);
  }, [query, actions.length]);

  useEffect(() => {
    if (filtered.length === 0) {
      setHighlight(0);
      return;
    }
    setHighlight((current) => Math.min(current, filtered.length - 1));
  }, [filtered.length]);

  const selected = filtered[highlight] ?? null;

  return (
    <div
      role="presentation"
      style={S.overlay}
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Copy As"
        style={S.dialog}
        onClick={(e) => e.stopPropagation()}
      >
        <div style={S.header}>
          <div style={S.titleBlock}>
            <div style={S.title}>Copy As</div>
            <div style={S.subtitle}>Choose a local transform and copy the result.</div>
          </div>
          <button
            type="button"
            aria-label="Close"
            style={S.closeBtn}
            onClick={onClose}
          >
            <IconX size={12} />
          </button>
        </div>

        <label style={S.searchRow}>
          <span style={S.searchIcon}><IconSearch size={13} /></span>
          <input
            ref={inputRef}
            aria-label="Copy As"
            value={query}
            onChange={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                e.preventDefault();
                onClose();
                return;
              }
              if (e.key === 'ArrowDown') {
                e.preventDefault();
                if (filtered.length === 0) return;
                setHighlight((current) => Math.min(current + 1, filtered.length - 1));
                return;
              }
              if (e.key === 'ArrowUp') {
                e.preventDefault();
                if (filtered.length === 0) return;
                setHighlight((current) => Math.max(current - 1, 0));
                return;
              }
              if (e.key === 'Enter') {
                e.preventDefault();
                if (selected) onSelect(selected.id);
              }
            }}
            placeholder="Filter actions"
            style={S.input}
          />
        </label>

        <div style={S.list} role="listbox" aria-label="Copy As actions">
          {filtered.length === 0 ? (
            <div style={S.empty}>No matching actions.</div>
          ) : (
            filtered.map((action, index) => (
              <button
                key={action.id}
                type="button"
                role="option"
                aria-selected={index === highlight}
                onMouseEnter={() => setHighlight(index)}
                onClick={() => onSelect(action.id)}
                style={{ ...S.row, ...(index === highlight ? S.rowActive : null) }}
              >
                <span style={S.rowLabel}>{action.label}</span>
                <span style={S.rowId}>{action.id}</span>
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}

const S: Record<string, CSSProperties> = {
  overlay: {
    position: 'fixed',
    inset: 0,
    zIndex: 250,
    background: 'rgba(0, 0, 0, 0.38)',
    display: 'flex',
    justifyContent: 'center',
    alignItems: 'flex-start',
    paddingTop: 72,
  },
  dialog: {
    ...dialogStyles.dialog,
    width: 'min(480px, calc(100vw - 32px))',
    maxWidth: 'min(480px, calc(100vw - 32px))',
    padding: 0,
    overflow: 'hidden',
  },
  header: {
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: 12,
    padding: '18px 18px 14px',
    borderBottom: `1px solid ${C.border}`,
  },
  titleBlock: {
    minWidth: 0,
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
  },
  title: {
    fontSize: 13,
    fontWeight: 600,
    color: C.t1,
    lineHeight: 1.2,
  },
  subtitle: {
    fontSize: 12,
    lineHeight: 1.4,
    color: C.t2,
  },
  closeBtn: {
    border: `1px solid ${C.border}`,
    background: C.card,
    color: C.t2,
    width: 24,
    height: 24,
    borderRadius: 6,
    display: 'grid',
    placeItems: 'center',
    cursor: 'pointer',
    flexShrink: 0,
  },
  searchRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    padding: '12px 18px',
    borderBottom: `1px solid ${C.border}`,
  },
  searchIcon: {
    color: C.t3,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    flexShrink: 0,
  },
  input: {
    width: '100%',
    border: 'none',
    outline: 'none',
    background: 'transparent',
    color: C.t1,
    fontSize: 14,
    fontFamily: 'inherit',
    minWidth: 0,
  },
  list: {
    maxHeight: 360,
    overflowY: 'auto',
    padding: 6,
  },
  row: {
    width: '100%',
    border: 'none',
    background: 'transparent',
    color: C.t1,
    borderRadius: 6,
    cursor: 'pointer',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    gap: 12,
    padding: '9px 10px',
    textAlign: 'left',
  },
  rowActive: {
    background: C.hover,
  },
  rowLabel: {
    fontSize: 13,
    fontWeight: 500,
    minWidth: 0,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    whiteSpace: 'nowrap',
  },
  rowId: {
    fontSize: 11,
    color: C.t3,
    fontFamily: 'var(--font-mono)',
    flexShrink: 0,
  },
  empty: {
    padding: '18px 10px 20px',
    fontSize: 13,
    color: C.t3,
  },
};
