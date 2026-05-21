import { sourcePillVars, type SourceColorSlot } from '../lib/sourceColor';

interface SourcePillProps {
  source: string; // "local" | "remote:hostname"
  status: 'local' | 'remote';
  nickname?: string;
  colorSlot?: SourceColorSlot;
}

export function SourcePill({ source, nickname, colorSlot }: SourcePillProps) {
  const label = nickname ?? (source.startsWith('remote:')
    ? source.replace('remote:', '')
    : source);

  const { bg, fg } = sourcePillVars(source, colorSlot);

  return (
    <span
      style={{
        display: 'inline-flex',
        alignItems: 'center',
        background: bg,
        color: fg,
        borderRadius: 9999,
        padding: '1px 8px',
        maxWidth: 230,
        overflow: 'hidden',
        fontSize: 10,
        fontFamily: 'var(--font-mono)',
        letterSpacing: '0.04em',
        textTransform: 'none',
        whiteSpace: 'nowrap',
        textOverflow: 'ellipsis',
        flexShrink: 0,
      }}
    >
      {label}
    </span>
  );
}
