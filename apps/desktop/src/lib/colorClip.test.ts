import { describe, expect, it, vi } from 'vitest';
import { parseColorClip } from './colorClip';

describe('parseColorClip', () => {
  it.each([
    '#fff',
    '#ffff',
    '#ffffff',
    '#ffffffff',
    'rgb(255, 255, 255)',
    'rgba(0, 128, 255, 0.5)',
    'rgb(255 255 255 / 50%)',
    'hsl(210, 50%, 40%)',
    'hsla(210 50% 40% / 0.75)',
    'rebeccapurple',
    'transparent',
  ])('detects %s as a color clip', (value) => {
    expect(parseColorClip(value)).toMatchObject({ value });
  });

  it.each([
    '',
    'n',
    '#ff',
    '#fffff',
    'rgb(300, 0, 0)',
    'rgba(0, 0, 0, 2)',
    'hsl(210, 120%, 40%)',
    'inherit',
    'background: #fff;',
    '#fff\n#000',
  ])('does not detect %s as a standalone color clip', (value) => {
    expect(parseColorClip(value)).toBeNull();
  });

  it('uses browser color support for modern CSS color functions', () => {
    const supports = vi.fn((_property: string, value: string) => value === 'oklch(70% 0.2 180)');
    vi.stubGlobal('CSS', { supports });

    expect(parseColorClip('oklch(70% 0.2 180)')).toMatchObject({
      value: 'oklch(70% 0.2 180)',
      format: 'css-function',
    });
    expect(parseColorClip('oklch(not a color)')).toBeNull();
    expect(supports).toHaveBeenCalledWith('color', 'oklch(70% 0.2 180)');

    vi.unstubAllGlobals();
  });
});
