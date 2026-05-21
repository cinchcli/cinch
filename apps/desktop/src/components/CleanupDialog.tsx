import { useEffect, useRef, useId, useState, type CSSProperties } from 'react';
import { C } from '../design';

interface CleanupDialogProps {
  open: boolean;
  hostname: string;
  onClose: () => void;
}

const DARK_SHADOW =
  'rgba(0,0,0,0.5) 0 0 0 2px, rgba(255,255,255,0.19) 0 0 14px, rgba(255,255,255,0.05) 0 1px 0 0 inset';

const COMMAND = 'cinch auth logout';

export function CleanupDialog({ open, hostname, onClose }: CleanupDialogProps) {
  const doneRef = useRef<HTMLButtonElement | null>(null);
  const titleId = useId();
  const bodyId = useId();
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener('keydown', h);
    return () => window.removeEventListener('keydown', h);
  }, [open, onClose]);

  useEffect(() => {
    if (open) {
      const raf = requestAnimationFrame(() => doneRef.current?.focus());
      return () => cancelAnimationFrame(raf);
    }
  }, [open]);

  useEffect(() => {
    if (!open) setCopied(false);
  }, [open]);

  if (!open) return null;

  const handleCopy = () => {
    navigator.clipboard.writeText(COMMAND).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  const styles: Record<string, CSSProperties> = {
    overlay: {
      position: 'fixed',
      inset: 0,
      background: 'rgba(0,0,0,0.55)',
      zIndex: 200,
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      animation: 'confirm-fade-in 200ms cubic-bezier(0.16,1,0.3,1)',
    },
    dialog: {
      background: C.card,
      border: '1px solid var(--border)',
      borderRadius: 12,
      maxWidth: 420,
      width: 'calc(100% - 48px)',
      padding: '24px 24px 16px',
      color: C.t1,
      boxShadow: DARK_SHADOW,
      animation:
        'confirm-enter 250ms cubic-bezier(0.16,1,0.3,1), confirm-fade-in 200ms cubic-bezier(0.16,1,0.3,1)',
    },
    title: {
      fontSize: 20,
      fontWeight: 500,
      lineHeight: 1.6,
      letterSpacing: '0.2px',
      color: C.t1,
      marginBottom: 8,
    },
    body: {
      fontSize: 14,
      fontWeight: 500,
      lineHeight: 1.55,
      color: C.t2,
      marginBottom: 16,
    },
    codeRow: {
      display: 'flex',
      alignItems: 'center',
      gap: 8,
      background: C.card2 ?? 'rgba(255,255,255,0.05)',
      border: `1px solid ${C.border}`,
      borderRadius: 8,
      padding: '10px 12px',
      marginBottom: 20,
    },
    code: {
      flex: 1,
      fontFamily: 'var(--font-mono)',
      fontSize: 13,
      color: C.t1,
      userSelect: 'text',
      letterSpacing: '0.02em',
    },
    copyBtn: {
      background: 'transparent',
      border: `1px solid ${C.border}`,
      color: copied ? C.accent : C.t3,
      fontSize: 11,
      fontWeight: 600,
      letterSpacing: '0.3px',
      padding: '4px 10px',
      borderRadius: 5,
      cursor: 'pointer',
      fontFamily: 'var(--font-body)',
      whiteSpace: 'nowrap',
      flexShrink: 0,
      transition: 'color 150ms',
    },
    actions: {
      display: 'flex',
      justifyContent: 'flex-end',
      gap: 8,
      marginTop: 8,
    },
    doneBtn: {
      background: C.t1,
      color: C.bg,
      border: 'none',
      fontSize: 12,
      fontWeight: 600,
      letterSpacing: '0.3px',
      padding: '8px 14px',
      borderRadius: 6,
      cursor: 'pointer',
    },
    hint: {
      fontSize: 12,
      fontWeight: 400,
      color: C.t3,
      marginTop: 12,
      textAlign: 'left' as const,
    },
  };

  return (
    <div style={styles.overlay} onClick={onClose} role="presentation">
      <div
        style={styles.dialog}
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={bodyId}
      >
        <h2 id={titleId} style={styles.title}>
          Clean up {hostname}
        </h2>
        <div id={bodyId} style={styles.body}>
          <strong>{hostname}</strong> has been revoked. To remove cinch data
          from that machine, run this command on it:
        </div>
        <div style={styles.codeRow}>
          <span style={styles.code}>{COMMAND}</span>
          <button
            type="button"
            style={styles.copyBtn}
            onClick={handleCopy}
            aria-label="Copy command"
          >
            {copied ? 'Copied!' : 'Copy'}
          </button>
        </div>
        <div style={styles.actions}>
          <button
            ref={doneRef}
            type="button"
            style={styles.doneBtn}
            onClick={onClose}
          >
            Done
          </button>
        </div>
        <div style={styles.hint}>Esc to close</div>
      </div>
      <style>{`
        @keyframes confirm-fade-in {
          from { opacity: 0; }
          to { opacity: 1; }
        }
        @keyframes confirm-enter {
          from { transform: translateY(8px); }
          to { transform: translateY(0); }
        }
        @media (prefers-reduced-motion: reduce) {
          @keyframes confirm-enter {
            from { transform: none; }
            to { transform: none; }
          }
        }
      `}</style>
    </div>
  );
}
