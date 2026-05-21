import { describe, it, expect } from 'vitest';
import { buildDeviceOptions } from './deviceOptions';
import type { Device, SourceInfo } from '../bindings';

const dev = (overrides: Partial<Device>): Device => ({
  id: 'd-' + Math.random().toString(36).slice(2, 8),
  ...overrides,
});

const src = (source: string, clip_count: number): SourceInfo => ({
  source,
  clip_count,
  last_seen: 0,
});

describe('buildDeviceOptions', () => {
  it('returns empty list when nothing is paired', () => {
    expect(buildDeviceOptions({
      devices: [], sources: [], displayNames: {}, tagColors: {},
    })).toEqual([]);
  });

  it('omits the local capture stream from both devices and sources', () => {
    const result = buildDeviceOptions({
      devices: [
        dev({ source_key: 'local', hostname: 'this-mac' }),
        dev({ source_key: 'remote:laptop', hostname: 'laptop' }),
      ],
      sources: [src('local', 44), src('remote:laptop', 12)],
      displayNames: {},
      tagColors: {},
    });
    expect(result).toEqual([
      { source: 'remote:laptop', label: 'laptop', count: 12, colorSlot: undefined },
    ]);
  });

  it('falls back through displayNames → nickname → hostname → trimmed source', () => {
    const result = buildDeviceOptions({
      devices: [
        dev({ source_key: 'remote:a', nickname: 'Alpha' }),
        dev({ source_key: 'remote:b', hostname: 'beta.local' }),
        dev({ source_key: 'remote:c' }),
      ],
      sources: [],
      displayNames: { 'remote:a': 'Renamed Alpha' },
      tagColors: {},
    });
    expect(result.map((o) => o.label).sort()).toEqual(['Renamed Alpha', 'beta.local', 'c']);
  });

  it('treats empty-string displayName / nickname as "use the next fallback"', () => {
    // Regression: `??` would have stopped at "" and rendered a blank chip;
    // `||` falls through to the next non-empty layer.
    const result = buildDeviceOptions({
      devices: [
        dev({ source_key: 'remote:laptop', nickname: '', hostname: 'laptop.local' }),
      ],
      sources: [],
      displayNames: { 'remote:laptop': '' },
      tagColors: {},
    });
    expect(result).toEqual([
      { source: 'remote:laptop', label: 'laptop.local', count: 0, colorSlot: undefined },
    ]);
  });

  it('uses the raw source key when every other fallback is empty', () => {
    const result = buildDeviceOptions({
      devices: [dev({ source_key: 'remote:' })],
      sources: [],
      displayNames: {},
      tagColors: {},
    });
    // trimRemote("remote:") = "" → final fallback is the raw source key
    expect(result[0].label).toBe('remote:');
  });

  it('merges device entries with their clip counts from sources', () => {
    const result = buildDeviceOptions({
      devices: [
        dev({ source_key: 'remote:a', hostname: 'a' }),
        dev({ source_key: 'remote:b', hostname: 'b' }),
      ],
      sources: [src('remote:a', 5), src('remote:b', 99)],
      displayNames: {},
      tagColors: {},
    });
    expect(result).toEqual([
      { source: 'remote:b', label: 'b', count: 99, colorSlot: undefined },
      { source: 'remote:a', label: 'a', count: 5,  colorSlot: undefined },
    ]);
  });

  it('includes sources that have clips but no matching device row yet', () => {
    const result = buildDeviceOptions({
      devices: [],
      sources: [src('remote:phantom', 3)],
      displayNames: {},
      tagColors: {},
    });
    expect(result).toEqual([
      { source: 'remote:phantom', label: 'phantom', count: 3, colorSlot: undefined },
    ]);
  });

  it('attaches color slots from tagColors when present', () => {
    const result = buildDeviceOptions({
      devices: [dev({ source_key: 'remote:laptop', hostname: 'laptop' })],
      sources: [],
      displayNames: {},
      tagColors: { 'remote:laptop': 'mint' },
    });
    expect(result[0].colorSlot).toBe('mint');
  });

  it('dedupes when devices and sources both mention the same source', () => {
    const result = buildDeviceOptions({
      devices: [dev({ source_key: 'remote:laptop', hostname: 'laptop' })],
      sources: [src('remote:laptop', 7)],
      displayNames: {},
      tagColors: {},
    });
    expect(result).toHaveLength(1);
    expect(result[0]).toMatchObject({ source: 'remote:laptop', count: 7 });
  });

  it('dedupes duplicate device entries', () => {
    const result = buildDeviceOptions({
      devices: [
        dev({ source_key: 'remote:laptop', hostname: 'laptop' }),
        dev({ source_key: 'remote:laptop', hostname: 'laptop' }),
      ],
      sources: [],
      displayNames: {},
      tagColors: {},
    });
    expect(result).toHaveLength(1);
  });

  it('skips devices missing a source_key', () => {
    const result = buildDeviceOptions({
      devices: [dev({ hostname: 'unpaired' })],
      sources: [],
      displayNames: {},
      tagColors: {},
    });
    expect(result).toEqual([]);
  });

  it('sorts by clip count desc, then label alpha', () => {
    const result = buildDeviceOptions({
      devices: [
        dev({ source_key: 'remote:a', hostname: 'a' }),
        dev({ source_key: 'remote:b', hostname: 'b' }),
        dev({ source_key: 'remote:c', hostname: 'c' }),
      ],
      sources: [src('remote:a', 5), src('remote:b', 5), src('remote:c', 10)],
      displayNames: {},
      tagColors: {},
    });
    expect(result.map((o) => o.source)).toEqual(['remote:c', 'remote:a', 'remote:b']);
  });
});
