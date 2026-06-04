import type { CSSProperties } from 'react';
import { C } from '../design';

// Shimmer placeholder shown while the inbox loads its first batch of clips, so
// the list resolves skeleton → clips instead of flashing the empty state and
// then popping the rows in. Mirrors the loading state in
// preview/redesign-mockups/states.html. Purely presentational (aria-hidden);
// the real ClipList component is untouched.
const ROWS: { meta: number; line: number }[] = [
  { meta: 38, line: 80 },
  { meta: 30, line: 66 },
  { meta: 44, line: 72 },
  { meta: 34, line: 58 },
  { meta: 40, line: 74 },
  { meta: 32, line: 60 },
];

export function ClipListSkeleton() {
  return (
    <div style={S.col} aria-hidden="true" data-testid="clip-list-skeleton">
      {ROWS.map((r, i) => (
        <div key={i} style={S.row}>
          <div className="skeleton-shimmer" style={{ ...S.bar, width: `${r.meta}%`, height: 9 }} />
          <div className="skeleton-shimmer" style={{ ...S.bar, width: `${r.line}%`, height: 12 }} />
        </div>
      ))}
    </div>
  );
}

const S: Record<string, CSSProperties> = {
  col: {
    width: 'var(--list-width, 320px)',
    flexShrink: 0,
    background: C.card,
    borderRight: `1px solid ${C.border}`,
    overflow: 'hidden',
  },
  row: {
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
    padding: 'var(--sp-md) var(--sp-lg)',
    borderBottom: `1px solid ${C.border}`,
  },
  bar: {
    borderRadius: 4,
    background: C.card2,
  },
};
