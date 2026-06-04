import { useState, type CSSProperties } from 'react';
import { C } from '../design';
import { IconCopy } from '../icons';

interface CopyFieldProps {
  /** The exact text shown in the box and written to the clipboard. */
  value: string;
  /** Accessible label for the copy button. */
  label?: string;
}

export function CopyField({ value, label }: CopyFieldProps) {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    navigator.clipboard.writeText(value).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  return (
    <div style={S.row}>
      <code style={S.code}>{value}</code>
      <button
        type="button"
        onClick={handleCopy}
        style={S.btn}
        aria-label={label ?? 'Copy command'}
      >
        {copied ? 'Copied' : <IconCopy size={14} />}
      </button>
    </div>
  );
}

const S: Record<string, CSSProperties> = {
  row: {
    display: 'flex',
    alignItems: 'stretch',
    gap: 8,
    marginTop: 6,
  },
  code: {
    flex: 1,
    minWidth: 0,
    background: C.card,
    border: `1px solid ${C.border}`,
    borderRadius: 6,
    color: C.t1,
    fontFamily: 'var(--font-mono)',
    fontSize: 12.5,
    lineHeight: 1.5,
    padding: '8px 12px',
    overflowX: 'auto',
    whiteSpace: 'pre',
  },
  btn: {
    flexShrink: 0,
    minWidth: 64,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    background: 'transparent',
    border: `1px solid ${C.border}`,
    borderRadius: 6,
    color: C.t2,
    fontSize: 11.5,
    fontWeight: 500,
    fontFamily: 'inherit',
    letterSpacing: '0.1px',
    cursor: 'pointer',
    padding: '0 10px',
  },
};
