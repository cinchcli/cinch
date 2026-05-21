export const MACHINE_DISPLAY_NAMES_STORAGE_KEY = 'cinch.machineDisplayNames.v1';
export const MACHINE_DISPLAY_NAMES_EVENT = 'cinch:machine-display-names-changed';

export type MachineDisplayNameMap = Record<string, string>;

export function loadMachineDisplayNames(): MachineDisplayNameMap {
  if (typeof window === 'undefined') return {};
  const raw = window.localStorage.getItem(MACHINE_DISPLAY_NAMES_STORAGE_KEY);
  if (!raw) return {};

  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const names: MachineDisplayNameMap = {};
    for (const [source, name] of Object.entries(parsed)) {
      if (source && typeof name === 'string' && name.trim()) {
        names[source] = name.trim();
      }
    }
    return names;
  } catch {
    return {};
  }
}

export function setMachineDisplayName(source: string, name: string | null): MachineDisplayNameMap {
  const next = loadMachineDisplayNames();
  const trimmed = name?.trim() ?? '';
  if (trimmed) {
    next[source] = trimmed;
  } else {
    delete next[source];
  }

  if (Object.keys(next).length === 0) {
    window.localStorage.removeItem(MACHINE_DISPLAY_NAMES_STORAGE_KEY);
  } else {
    window.localStorage.setItem(MACHINE_DISPLAY_NAMES_STORAGE_KEY, JSON.stringify(next));
  }

  window.dispatchEvent(new CustomEvent<MachineDisplayNameMap>(MACHINE_DISPLAY_NAMES_EVENT, { detail: next }));
  return next;
}
