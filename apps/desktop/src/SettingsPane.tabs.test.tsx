import { describe, it, expect } from 'vitest';
import { SETTINGS_TABS, CATEGORY_META } from './SettingsPane';

describe('Settings IA', () => {
  it('exposes exactly the five new tabs in order', () => {
    expect(SETTINGS_TABS).toEqual(['general', 'privacy', 'devices', 'agents', 'shortcuts']);
  });
  it('labels the new section "Agents & CLI" with correct casing', () => {
    expect(CATEGORY_META.agents.label).toBe('Agents & CLI');
  });
  it('no longer has account or sessions/servers tabs', () => {
    expect(SETTINGS_TABS).not.toContain('account');
    expect(SETTINGS_TABS).not.toContain('servers');
    expect(SETTINGS_TABS).not.toContain('sessions');
  });
});
