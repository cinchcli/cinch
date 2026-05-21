import { describe, it, expect } from 'vitest';
import { groupByTimeBucket, type TimeBucket } from './timeBuckets';

const NOW = new Date('2026-05-01T12:00:00Z').getTime() / 1000;

const clip = (id: string, secondsAgo: number) => ({
  id,
  created_at: NOW - secondsAgo,
});

describe('groupByTimeBucket', () => {
  it('returns empty array for empty input', () => {
    expect(groupByTimeBucket([], NOW)).toEqual([]);
  });

  it('puts clips < 24h ago into Today', () => {
    const result = groupByTimeBucket([clip('a', 60), clip('b', 3600 * 5)], NOW);
    expect(result).toEqual([
      { bucket: 'Today', items: [clip('a', 60), clip('b', 3600 * 5)] },
    ]);
  });

  it('puts clips 24-48h ago into Yesterday', () => {
    const result = groupByTimeBucket([clip('a', 60), clip('b', 3600 * 30)], NOW);
    expect(result).toEqual([
      { bucket: 'Today', items: [clip('a', 60)] },
      { bucket: 'Yesterday', items: [clip('b', 3600 * 30)] },
    ]);
  });

  it('puts clips 48h-7d into This week', () => {
    const result = groupByTimeBucket([clip('a', 3600 * 72), clip('b', 3600 * 24 * 6)], NOW);
    expect(result).toEqual([
      { bucket: 'This week', items: [clip('a', 3600 * 72), clip('b', 3600 * 24 * 6)] },
    ]);
  });

  it('puts clips > 7d into Older', () => {
    const result = groupByTimeBucket([clip('a', 3600 * 24 * 10)], NOW);
    expect(result).toEqual([
      { bucket: 'Older', items: [clip('a', 3600 * 24 * 10)] },
    ]);
  });

  it('preserves input order within each bucket', () => {
    const result = groupByTimeBucket(
      [clip('a', 100), clip('b', 200), clip('c', 50)],
      NOW
    );
    expect(result[0].items.map(c => c.id)).toEqual(['a', 'b', 'c']);
  });

  it('uses received_at for recency when a historical clip was copied again', () => {
    const oldCopiedAgain = {
      id: 'old',
      created_at: NOW - 3600 * 24 * 30,
      received_at: NOW - 60,
    };
    const result = groupByTimeBucket([oldCopiedAgain], NOW);
    expect(result).toEqual([
      { bucket: 'Today', items: [oldCopiedAgain] },
    ]);
  });

  it('returns buckets in chronological order: Today, Yesterday, This week, Older', () => {
    const result = groupByTimeBucket(
      [
        clip('older', 3600 * 24 * 30),
        clip('today', 60),
        clip('week', 3600 * 24 * 4),
        clip('yesterday', 3600 * 30),
      ],
      NOW
    );
    expect(result.map(g => g.bucket)).toEqual(['Today', 'Yesterday', 'This week', 'Older']);
  });
});

// Type-checking aid: ensure exported TimeBucket type is what we expect
const _typeCheck: TimeBucket[] = ['Today', 'Yesterday', 'This week', 'Older'];
void _typeCheck;
