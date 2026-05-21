import { describe, expect, it } from 'vitest';
import type { LocalClip } from '../bindings';
import { buildTargets, fuzzySearch, parseFromToken } from './fuzzy';

function clip(partial: Partial<LocalClip>): LocalClip {
  return {
    id: partial.id ?? 'id',
    user_id: partial.user_id ?? 'u',
    content: partial.content ?? '',
    content_type: partial.content_type ?? 'text',
    source: partial.source ?? 'local',
    label: partial.label ?? '',
    byte_size: partial.byte_size ?? 0,
    media_path: partial.media_path ?? null,
    created_at: partial.created_at ?? 0,
    synced: partial.synced ?? true,
    is_pinned: partial.is_pinned ?? false,
    pin_note: partial.pin_note ?? null,
  };
}

describe('parseFromToken', () => {
  it('extracts from:nickname and returns the residual query', () => {
    expect(parseFromToken('from:laptop hello world')).toEqual({
      from: 'laptop',
      residual: 'hello world',
    });
  });

  it('returns null from when no token present', () => {
    expect(parseFromToken('hello world')).toEqual({
      from: null,
      residual: 'hello world',
    });
  });

  it('handles trailing/leading whitespace in residual', () => {
    expect(parseFromToken('  hello  from:mbp  there  ')).toEqual({
      from: 'mbp',
      residual: 'hello there',
    });
  });

  it('is case-insensitive on the token name', () => {
    expect(parseFromToken('FROM:Laptop x')).toEqual({
      from: 'Laptop',
      residual: 'x',
    });
  });
});

describe('buildTargets', () => {
  it('joins content + nickname into haystack', () => {
    const c = clip({ id: '1', content: 'hello', source: 'remote:mbp' });
    const targets = buildTargets([c], { 'remote:mbp': 'MBP-Pro' });
    expect(targets).toHaveLength(1);
    expect(targets[0].haystack).toContain('hello');
    expect(targets[0].haystack).toContain('MBP-Pro');
  });

  it('includes pin_note when includePinNote is true', () => {
    const c = clip({ id: '1', content: 'body', pin_note: 'launch codes' });
    const [t] = buildTargets([c], {}, true);
    expect(t.haystack).toContain('launch codes');
  });

  it('omits pin_note when includePinNote is false (default)', () => {
    const c = clip({ id: '1', content: 'body', pin_note: 'launch codes' });
    const [t] = buildTargets([c], {});
    expect(t.haystack).not.toContain('launch codes');
  });
});

describe('fuzzySearch', () => {
  const targets = buildTargets(
    [
      clip({ id: 'a', content: 'my password is hunter2', source: 'remote:mbp' }),
      clip({ id: 'b', content: 'paste this into terminal', source: 'remote:linux-box' }),
      clip({ id: 'c', content: 'unrelated banana', source: 'local' }),
    ],
    { 'remote:mbp': 'MBP-Pro', 'remote:linux-box': 'tux' },
  );

  it('returns all clips when query is empty', () => {
    expect(fuzzySearch(targets, '').map(c => c.id)).toEqual(['a', 'b', 'c']);
  });

  it('matches substrings in content', () => {
    const ids = fuzzySearch(targets, 'pass').map(c => c.id);
    expect(ids).toContain('a');
    expect(ids).not.toContain('c');
  });

  it('matches against device nickname', () => {
    const ids = fuzzySearch(targets, 'tux').map(c => c.id);
    expect(ids).toEqual(['b']);
  });

  it('returns empty array when nothing matches', () => {
    expect(fuzzySearch(targets, 'xyzzy')).toEqual([]);
  });

  it('is case-insensitive', () => {
    const ids = fuzzySearch(targets, 'PASTE').map(c => c.id);
    expect(ids).toContain('b');
  });
});
