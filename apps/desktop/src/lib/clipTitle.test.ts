import { describe, it, expect } from 'vitest';
import { clipTitle } from './clipTitle';

describe('clipTitle', () => {
  it('returns "Image (size)" for image clips', () => {
    expect(clipTitle({ content_type: 'image', content: '', byte_size: 2048, source: 'local' }))
      .toBe('Image (2.0 KB)');
    expect(clipTitle({ content_type: 'image', content: '', byte_size: 500, source: 'local' }))
      .toBe('Image (500 B)');
  });

  it('returns first 60 chars of content for text clips, single line', () => {
    expect(clipTitle({
      content_type: 'text',
      content: 'hello world',
      byte_size: 11,
      source: 'local',
    })).toBe('hello world');
  });

  it('collapses whitespace and trims for text', () => {
    expect(clipTitle({
      content_type: 'text',
      content: '  multi\n   line\t with whitespace ',
      byte_size: 33,
      source: 'local',
    })).toBe('multi line with whitespace');
  });

  it('truncates long content to 60 chars + ellipsis', () => {
    const long = 'a'.repeat(100);
    const out = clipTitle({ content_type: 'text', content: long, byte_size: 100, source: 'local' });
    expect(out.length).toBeLessThanOrEqual(61);
    expect(out.endsWith('…')).toBe(true);
  });

  it('uses content for json/url/code clips, not the type name', () => {
    expect(clipTitle({
      content_type: 'json',
      content: '{"hello":"world"}',
      byte_size: 17,
      source: 'local',
    })).toBe('{"hello":"world"}');
  });

  it('returns "(empty clip)" when content is blank and not image', () => {
    expect(clipTitle({ content_type: 'text', content: '', byte_size: 0, source: 'local' }))
      .toBe('(empty clip)');
    expect(clipTitle({ content_type: 'text', content: '   \n\t  ', byte_size: 6, source: 'local' }))
      .toBe('(empty clip)');
  });
});
