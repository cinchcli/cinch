export const PILL_PALETTE_SIZE = 6;

export interface PillVars {
  bg: string;
  fg: string;
}

export type SourceColorSlot = 'mint' | 'amber' | 'sky' | 'lilac' | 'rose' | 'sage';

export const SOURCE_COLOR_OPTIONS: Array<{ value: SourceColorSlot; label: string }> = [
  { value: 'mint', label: 'Mint' },
  { value: 'amber', label: 'Amber' },
  { value: 'sky', label: 'Sky' },
  { value: 'lilac', label: 'Lilac' },
  { value: 'rose', label: 'Rose' },
  { value: 'sage', label: 'Sage' },
];

const SOURCE_COLOR_SLOT_INDEX: Record<SourceColorSlot, number> = {
  mint: 1,
  amber: 2,
  sky: 3,
  lilac: 4,
  rose: 5,
  sage: 6,
};

export function sourceColorSlotVars(slot: SourceColorSlot): PillVars {
  const index = SOURCE_COLOR_SLOT_INDEX[slot];
  return {
    bg: `var(--pill-${index}-bg)`,
    fg: `var(--pill-${index}-fg)`,
  };
}

/**
 * Maps a clip source key to a CSS variable pair for its pill background and foreground.
 * `local` always returns the neutral pill. Remote sources hash deterministically into
 * one of PILL_PALETTE_SIZE color slots so the same machine always reads in the same color.
 */
export function sourcePillVars(source: string, colorSlot?: SourceColorSlot | null): PillVars {
  if (source === 'local') {
    return { bg: 'var(--pill-local-bg)', fg: 'var(--pill-local-fg)' };
  }
  if (colorSlot) {
    return sourceColorSlotVars(colorSlot);
  }
  let hash = 0;
  for (let i = 0; i < source.length; i++) {
    hash = (hash * 31 + source.charCodeAt(i)) >>> 0;
  }
  const slot = (hash % PILL_PALETTE_SIZE) + 1;
  return {
    bg: `var(--pill-${slot}-bg)`,
    fg: `var(--pill-${slot}-fg)`,
  };
}
