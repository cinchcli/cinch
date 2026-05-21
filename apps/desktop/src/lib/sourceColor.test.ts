import { describe, it, expect } from 'vitest';
import { sourcePillVars, PILL_PALETTE_SIZE } from './sourceColor';

describe('sourcePillVars', () => {
  it('returns local pill vars for "local"', () => {
    expect(sourcePillVars('local')).toEqual({
      bg: 'var(--pill-local-bg)',
      fg: 'var(--pill-local-fg)',
    });
  });

  it('returns one of the 6 palette slots for any non-local source', () => {
    const v = sourcePillVars('remote:prod-shell');
    const allowed = Array.from({ length: PILL_PALETTE_SIZE }, (_, i) => ({
      bg: `var(--pill-${i + 1}-bg)`,
      fg: `var(--pill-${i + 1}-fg)`,
    }));
    expect(allowed).toContainEqual(v);
  });

  it('is deterministic: same input → same slot', () => {
    expect(sourcePillVars('remote:prod-shell')).toEqual(sourcePillVars('remote:prod-shell'));
    expect(sourcePillVars('remote:macbook')).toEqual(sourcePillVars('remote:macbook'));
  });

  it('different inputs hash to (usually) different slots', () => {
    const slots = new Set([
      'remote:prod-shell',
      'remote:macbook',
      'remote:ci-runner-3',
      'remote:dev-laptop',
      'remote:staging',
    ].map(s => JSON.stringify(sourcePillVars(s))));
    expect(slots.size).toBeGreaterThanOrEqual(3);
  });

  it('uses an explicit palette slot when a machine tag color is set', () => {
    expect(sourcePillVars('remote:prod-shell', 'amber')).toEqual({
      bg: 'var(--pill-2-bg)',
      fg: 'var(--pill-2-fg)',
    });
  });
});
