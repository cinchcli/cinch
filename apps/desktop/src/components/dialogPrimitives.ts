import type { CSSProperties } from 'react';
import { C } from '../design';

export const dialogStyles: Record<string, CSSProperties> = {
  overlay: {
    position: 'fixed',
    inset: 0,
    background: 'rgba(0, 0, 0, 0.55)',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    zIndex: 100,
  },
  dialog: {
    background: C.card,
    border: `1px solid ${C.border}`,
    borderRadius: 8,
    padding: 18,
    maxWidth: 380,
    width: '100%',
    boxShadow: 'var(--dialog-shadow)',
  },
  title: { fontSize: 13, fontWeight: 600, color: C.t1, marginBottom: 6 },
  body: { fontSize: 12, color: C.t2, marginBottom: 16, lineHeight: 1.55 },
  actions: { display: 'flex', gap: 8, justifyContent: 'flex-end' },
  btnPrimary: {
    padding: '6px 14px',
    borderRadius: 5,
    fontSize: 12,
    fontWeight: 500,
    background: C.t1,
    color: C.bg,
    border: 'none',
    cursor: 'pointer',
    fontFamily: 'inherit',
  },
  btnGhost: {
    padding: '6px 14px',
    borderRadius: 5,
    fontSize: 12,
    fontWeight: 500,
    background: 'transparent',
    color: C.t2,
    border: `1px solid ${C.border}`,
    cursor: 'pointer',
    fontFamily: 'inherit',
  },
};
