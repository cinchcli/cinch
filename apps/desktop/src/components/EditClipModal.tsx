import { useEffect, useRef, useState, useId, type CSSProperties } from 'react';
import type { LocalClip } from '../bindings';
import { C } from '../design';

interface EditClipModalProps {
  clip: LocalClip;
  onSave: (newContent: string) => void;
  onCancel: () => void;
}

const DARK_SHADOW =
  'rgba(0,0,0,0.5) 0 0 0 2px, rgba(255,255,255,0.19) 0 0 14px, rgba(255,255,255,0.05) 0 1px 0 0 inset';

// Static — every value derives from module-level tokens, so the object is
// hoisted out of the component to avoid rebuilding it on each keystroke.
const styles: Record<string, CSSProperties> = {
  overlay: {
    position: 'fixed', inset: 0, background: 'rgba(0,0,0,0.55)', zIndex: 200,
    display: 'flex', alignItems: 'center', justifyContent: 'center',
  },
  dialog: {
    background: C.card, border: '1px solid var(--border)', borderRadius: 12,
    maxWidth: 560, width: 'calc(100% - 48px)', maxHeight: 'calc(100vh - 80px)',
    overflow: 'hidden', padding: '20px 20px 14px',
    color: C.t1, boxShadow: DARK_SHADOW, display: 'flex', flexDirection: 'column', gap: 12,
  },
  title: { fontSize: 16, fontWeight: 500, color: C.t1, margin: 0 },
  textarea: {
    width: '100%', minHeight: 200, maxHeight: '60vh', resize: 'vertical',
    overflow: 'auto', boxSizing: 'border-box',
    background: C.card2, border: `1px solid ${C.border}`, borderRadius: 6,
    padding: 12, color: C.t1, fontFamily: 'var(--font-mono)', fontSize: 13, lineHeight: 1.6,
  },
  actions: { display: 'flex', justifyContent: 'flex-end', gap: 8 },
  secondaryBtn: {
    background: 'transparent', border: `1px solid ${C.borderHover}`, color: C.t1,
    fontSize: 12, fontWeight: 600, padding: '8px 14px', borderRadius: 6, cursor: 'pointer',
  },
  primaryBtn: {
    background: C.accent, color: C.accentOn, border: 'none',
    fontSize: 12, fontWeight: 600, padding: '8px 14px', borderRadius: 6, cursor: 'pointer',
  },
  hint: { fontSize: 11, color: C.t3, marginTop: 2 },
};

export function EditClipModal({ clip, onSave, onCancel }: EditClipModalProps) {
  const [text, setText] = useState(clip.content);
  const textRef = useRef<HTMLTextAreaElement | null>(null);
  const titleId = useId();

  useEffect(() => {
    const raf = requestAnimationFrame(() => textRef.current?.focus());
    return () => cancelAnimationFrame(raf);
  }, []);

  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.key === 'Escape') { e.preventDefault(); onCancel(); }
    };
    window.addEventListener('keydown', h);
    return () => window.removeEventListener('keydown', h);
  }, [onCancel]);

  return (
    <div style={styles.overlay} onClick={onCancel} role="presentation">
      <div
        style={styles.dialog}
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
      >
        <h2 id={titleId} style={styles.title}>Edit clip</h2>
        <textarea
          ref={textRef}
          style={styles.textarea}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              onSave(text);
            }
          }}
          aria-label="Clip content"
        />
        <div style={styles.actions}>
          <button type="button" style={styles.secondaryBtn} onClick={onCancel}>Cancel</button>
          <button type="button" style={styles.primaryBtn} onClick={() => onSave(text)}>
            Save &amp; Copy
          </button>
        </div>
        <div style={styles.hint}>⌘↵ to save &amp; copy · Esc to cancel</div>
      </div>
    </div>
  );
}
