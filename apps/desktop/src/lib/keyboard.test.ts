import { describe, it, expect } from 'vitest';
import { isImeComposition } from './keyboard';

describe('isImeComposition', () => {
  it('is true while an IME composition is in progress', () => {
    expect(isImeComposition({ isComposing: true, keyCode: 13 })).toBe(true);
  });
  it('is true for the legacy 229 keyCode', () => {
    expect(isImeComposition({ isComposing: false, keyCode: 229 })).toBe(true);
  });
  it('is false for a normal key', () => {
    expect(isImeComposition({ isComposing: false, keyCode: 13 })).toBe(false);
  });
});
