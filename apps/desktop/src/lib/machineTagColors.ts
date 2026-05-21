import type { SourceColorSlot } from './sourceColor';

export const MACHINE_TAG_COLORS_STORAGE_KEY = 'cinch.machineTagColors.v1';
export const MACHINE_TAG_COLORS_EVENT = 'cinch:machine-tag-colors-changed';

export type MachineTagColorMap = Record<string, SourceColorSlot>;

const VALID_SOURCE_COLOR_SLOTS = new Set<SourceColorSlot>([
  'mint',
  'amber',
  'sky',
  'lilac',
  'rose',
  'sage',
]);

function isSourceColorSlot(value: unknown): value is SourceColorSlot {
  return typeof value === 'string' && VALID_SOURCE_COLOR_SLOTS.has(value as SourceColorSlot);
}

export function loadMachineTagColors(): MachineTagColorMap {
  if (typeof window === 'undefined') return {};
  const raw = window.localStorage.getItem(MACHINE_TAG_COLORS_STORAGE_KEY);
  if (!raw) return {};

  try {
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const colors: MachineTagColorMap = {};
    for (const [source, color] of Object.entries(parsed)) {
      if (source && isSourceColorSlot(color)) {
        colors[source] = color;
      }
    }
    return colors;
  } catch {
    return {};
  }
}

export function setMachineTagColor(source: string, color: SourceColorSlot | null): MachineTagColorMap {
  const next = loadMachineTagColors();
  if (color) {
    next[source] = color;
  } else {
    delete next[source];
  }

  if (Object.keys(next).length === 0) {
    window.localStorage.removeItem(MACHINE_TAG_COLORS_STORAGE_KEY);
  } else {
    window.localStorage.setItem(MACHINE_TAG_COLORS_STORAGE_KEY, JSON.stringify(next));
  }

  window.dispatchEvent(new CustomEvent<MachineTagColorMap>(MACHINE_TAG_COLORS_EVENT, { detail: next }));
  return next;
}
