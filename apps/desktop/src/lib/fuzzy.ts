import uFuzzy from '@leeoniya/ufuzzy';
import type { LocalClip } from '../bindings';

export interface FuzzyTarget {
  clip: LocalClip;
  haystack: string;
}

const FROM_TOKEN_RE = /\bfrom:(\S+)/i;

export interface ParsedQuery {
  from: string | null;
  residual: string;
}

export function parseFromToken(query: string): ParsedQuery {
  const m = query.match(FROM_TOKEN_RE);
  if (!m) return { from: null, residual: query.trim() };
  const residual = query.replace(FROM_TOKEN_RE, ' ').replace(/\s+/g, ' ').trim();
  return { from: m[1], residual };
}

export function buildTargets(
  clips: LocalClip[],
  nicknameBySource: Record<string, string>,
  includePinNote = false,
): FuzzyTarget[] {
  return clips.map((clip) => {
    const nickname = nicknameBySource[clip.source] ?? '';
    const sourceTail = clip.source.startsWith('remote:')
      ? clip.source.slice('remote:'.length)
      : clip.source;
    const note = includePinNote && clip.pin_note ? clip.pin_note : '';
    return {
      clip,
      haystack: [clip.content, nickname, sourceTail, note]
        .filter((s) => s.length > 0)
        .join('  '),
    };
  });
}

const fuzz = new uFuzzy({
  intraMode: 1, // tolerate one typo per term
  intraIns: 1,  // one skipped char per term (fzf-lite)
});

// Raycast-style priority tiers
const enum MatchPriority {
  ContentPrefix = 0,   // content starts with query
  ContentWordStart = 1, // any word in content starts with query
  ContentContains = 2,  // content contains query (substring)
  HaystackContains = 3, // other fields (nickname/source/note) contain query
  FuzzyOnly = 4,        // fuzzy match only
}

function scoreTarget(contentLower: string, haystackLower: string, lower: string): MatchPriority {
  if (contentLower.startsWith(lower)) return MatchPriority.ContentPrefix;
  if (contentLower.split(/\s+/).some((w) => w.startsWith(lower))) return MatchPriority.ContentWordStart;
  if (contentLower.includes(lower)) return MatchPriority.ContentContains;
  if (haystackLower.includes(lower)) return MatchPriority.HaystackContains;
  return MatchPriority.FuzzyOnly;
}

export function fuzzySearch(targets: FuzzyTarget[], query: string): LocalClip[] {
  const trimmed = query.trim();
  if (!trimmed) return targets.map((t) => t.clip);

  const lower = trimmed.toLowerCase();
  const haystacks = targets.map((t) => t.haystack);

  // Get fuzzy-ranked indices (preserves ufuzzy relevance ordering)
  const rawIdxs = fuzz.filter(haystacks, trimmed);
  const fuzzyRankedIdxs: number[] = rawIdxs
    ? (() => {
        const info = fuzz.info(rawIdxs, haystacks, trimmed);
        const order = fuzz.sort(info, haystacks, trimmed);
        return order.map((rank) => info.idx[rank]);
      })()
    : [];

  // Map idx → fuzzy rank for stable secondary sort
  const fuzzyRankMap = new Map<number, number>();
  fuzzyRankedIdxs.forEach((idx, pos) => fuzzyRankMap.set(idx, pos));

  // Include substring matches that fuzzy might have missed
  haystacks.forEach((h, i) => {
    if (!fuzzyRankMap.has(i) && h.toLowerCase().includes(lower)) {
      fuzzyRankMap.set(i, fuzzyRankedIdxs.length + i);
    }
  });

  if (fuzzyRankMap.size === 0) return [];

  // Score and re-rank with Raycast-style priority
  const scored = Array.from(fuzzyRankMap.entries()).map(([idx, fuzzyRank]) => {
    const contentLower = targets[idx].clip.content.toLowerCase();
    const haystackLower = haystacks[idx].toLowerCase();
    return { idx, priority: scoreTarget(contentLower, haystackLower, lower), fuzzyRank };
  });

  // Primary: priority tier — Secondary: original fuzzy rank (preserves ufuzzy order within tier)
  scored.sort((a, b) => a.priority - b.priority || a.fuzzyRank - b.fuzzyRank);

  return scored.map(({ idx }) => targets[idx].clip);
}
