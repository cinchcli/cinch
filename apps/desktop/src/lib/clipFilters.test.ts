import { describe, expect, it } from 'vitest';
import {
  applyClipFilter,
} from './clipFilters';
import type { LocalClip } from '../bindings';

const clip = (content_type: string): LocalClip => ({
  id: content_type,
  user_id: 'u',
  content: content_type,
  content_type,
  source: 'local',
  label: '',
  byte_size: content_type.length,
  media_path: null,
  created_at: 1,
  synced: false,
  is_pinned: false,
  pin_note: null,
  received_at: 1,
});

describe('clip filter rules', () => {
  it('keeps the existing default filter behavior', () => {
    const clips = ['text', 'json', 'error', 'image', 'code', 'url'].map(clip);

    expect(applyClipFilter(clips, 'all').map((c) => c.content_type))
      .toEqual(['text', 'json', 'error', 'image', 'code', 'url']);
    expect(applyClipFilter(clips, 'text').map((c) => c.content_type))
      .toEqual(['text', 'json']);
    expect(applyClipFilter(clips, 'image').map((c) => c.content_type))
      .toEqual(['image']);
    expect(applyClipFilter(clips, 'code').map((c) => c.content_type))
      .toEqual(['code']);
    expect(applyClipFilter(clips, 'url').map((c) => c.content_type))
      .toEqual(['url']);
  });
});
