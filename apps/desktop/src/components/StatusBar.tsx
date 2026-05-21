import type { CSSProperties } from 'react';
import { C } from '../design';

interface StatusBarProps {
  clipCount: number;
  devicesOnline?: number;
  devicesTotal?: number;
  hints: { keys: string; label: string }[];
  onMouseDown?: (e: React.MouseEvent) => void;
}

export function StatusBar({
  clipCount,
  devicesOnline,
  devicesTotal,
  hints,
  onMouseDown,
}: StatusBarProps) {
  return (
    <footer style={S.bar} role="contentinfo" onMouseDown={onMouseDown}>
      <div style={S.left}>
        <span>{clipCount} {clipCount === 1 ? 'clip' : 'clips'}</span>
        {devicesTotal !== undefined && (
          <>
            <span style={{ color: C.t4 }}>·</span>
            <span>{devicesOnline ?? 0}/{devicesTotal} online</span>
          </>
        )}
      </div>
      <div style={S.right}>
        {hints.map((h) => (
          <span key={h.keys} style={S.hint}>
            <kbd style={S.kbd}>{h.keys}</kbd>
            <span style={S.hintLabel}>{h.label}</span>
          </span>
        ))}
      </div>
    </footer>
  );
}

const S: Record<string, CSSProperties> = {
  bar: {
    display: 'flex',
    alignItems: 'center',
    height: 28,
    padding: '0 var(--sp-lg)',
    gap: 'var(--sp-md)',
    background: C.bg,
    borderTop: `1px solid ${C.border}`,
    flexShrink: 0,
    fontFamily: 'var(--font-mono)',
    fontSize: 10.5,
    letterSpacing: '0.04em',
    color: C.t3,
  },
  left: { display: 'flex', alignItems: 'center', gap: 6 },
  right: { marginLeft: 'auto', display: 'flex', alignItems: 'center', gap: 14 },
  hint: { display: 'flex', alignItems: 'center', gap: 4 },
  kbd: {
    fontFamily: 'var(--font-mono)',
    fontSize: 9.5,
    padding: '1px 5px',
    background: 'var(--kbd-bg)',
    border: '1px solid var(--kbd-border)',
    color: 'var(--kbd-color)',
    borderRadius: 3,
    letterSpacing: '0.04em',
  },
  hintLabel: { color: C.t3 },
};
