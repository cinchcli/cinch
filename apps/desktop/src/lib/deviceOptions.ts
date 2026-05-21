import type { Device, SourceInfo } from '../bindings';
import type { DeviceOption } from '../components/SearchBar';
import type { MachineDisplayNameMap } from './machineDisplayNames';
import type { MachineTagColorMap } from './machineTagColors';

const LOCAL_SOURCE = 'local';
const REMOTE_PREFIX = 'remote:';

function trimRemote(source: string): string {
  return source.startsWith(REMOTE_PREFIX) ? source.slice(REMOTE_PREFIX.length) : source;
}

// Picks the best label for a device picker entry. Uses `||` (not `??`) so an
// empty string from any layer falls through — `displayNames` and the backend
// can both legitimately produce `""` when a user clears a nickname.
function resolveLabel(
  source: string,
  displayNames: MachineDisplayNameMap,
  nickname?: string,
  hostname?: string,
): string {
  return (
    displayNames[source] ||
    nickname ||
    hostname ||
    trimRemote(source) ||
    source
  );
}

/**
 * Builds the union of paired devices + clip-bearing sources into a flat
 * picker list. The local capture stream is omitted on purpose — the picker
 * is for filtering remote sources, and `local` is "this Mac" rather than a
 * device the user thinks of as switchable.
 */
export function buildDeviceOptions(args: {
  devices: Device[];
  sources: SourceInfo[];
  displayNames: MachineDisplayNameMap;
  tagColors: MachineTagColorMap;
}): DeviceOption[] {
  const { devices, sources, displayNames, tagColors } = args;

  const countBySource: Record<string, number> = {};
  for (const s of sources) countBySource[s.source] = s.clip_count;

  const options: DeviceOption[] = [];
  const seen = new Set<string>();

  for (const d of devices) {
    if (!d.source_key) continue;
    if (d.source_key === LOCAL_SOURCE) continue;
    if (seen.has(d.source_key)) continue;
    seen.add(d.source_key);
    options.push({
      source: d.source_key,
      label: resolveLabel(d.source_key, displayNames, d.nickname, d.hostname),
      count: countBySource[d.source_key] ?? 0,
      colorSlot: tagColors[d.source_key],
    });
  }

  // Catch sources that have clips but no matching device row yet (the two
  // refreshes can land out of order during sign-in).
  for (const s of sources) {
    if (s.source === LOCAL_SOURCE) continue;
    if (seen.has(s.source)) continue;
    seen.add(s.source);
    options.push({
      source: s.source,
      label: resolveLabel(s.source, displayNames),
      count: s.clip_count,
      colorSlot: tagColors[s.source],
    });
  }

  return options.sort((a, b) => b.count - a.count || a.label.localeCompare(b.label));
}
