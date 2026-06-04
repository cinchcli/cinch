import { sourcePillVars, type SourceColorSlot } from '../lib/sourceColor';

interface SourcePillProps {
  source: string; // "local" | "remote:hostname"
  status: 'local' | 'remote';
  nickname?: string;
  colorSlot?: SourceColorSlot;
}

// Source identity reads through a soft per-device color (the brand-adjacent pill
// palette): a subtle tinted chip + colored label. The color is derived from the
// source key (so a machine always reads in the same color) or an explicit slot.
// `status` stays in the prop shape for call-site compatibility.
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
        maxWidth: 230,
        overflow: 'hidden',
        fontSize: 10,
        fontFamily: 'var(--font-mono)',
        letterSpacing: '0.04em',
        whiteSpace: 'nowrap',
        textOverflow: 'ellipsis',
        flexShrink: 0,
        padding: '1px 7px',
        borderRadius: 999,
      }}
    >
      {label}
    </span>
  );
}
