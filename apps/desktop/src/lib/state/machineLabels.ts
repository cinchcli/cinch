import { useCallback, useEffect, useState } from "react";

import type { SourceColorSlot } from "../sourceColor";
import {
  loadMachineTagColors,
  setMachineTagColor,
  MACHINE_TAG_COLORS_EVENT,
  type MachineTagColorMap,
} from "../machineTagColors";
import {
  loadMachineDisplayNames,
  setMachineDisplayName,
  MACHINE_DISPLAY_NAMES_EVENT,
  type MachineDisplayNameMap,
} from "../machineDisplayNames";

export interface MachineLabels {
  tagColors: MachineTagColorMap;
  displayNames: MachineDisplayNameMap;
  /** Persist a per-source tag color (or clear it) and update local state. */
  setTagColor: (source: string, color: SourceColorSlot | null) => void;
  /** Persist a per-source display name (or clear it) and update local state. */
  setDisplayName: (source: string, name: string | null) => void;
}

/**
 * Shared machine-label state (per-source tag colors + display names) backed by
 * localStorage. Both `App` and `DevicesPanel` rendered identical copies of this
 * state + the cross-tab/cross-component sync effects; this hook is the single
 * source of truth. Writes go through `setMachineTagColor` / `setMachineDisplayName`
 * (which persist and broadcast the change event); every mounted consumer then
 * refreshes via the event + `storage` listeners below.
 */
export function useMachineLabels(): MachineLabels {
  const [tagColors, setTagColors] = useState<MachineTagColorMap>(() =>
    loadMachineTagColors(),
  );
  const [displayNames, setDisplayNames] = useState<MachineDisplayNameMap>(() =>
    loadMachineDisplayNames(),
  );

  useEffect(() => {
    const refresh = () => setTagColors(loadMachineTagColors());
    window.addEventListener(MACHINE_TAG_COLORS_EVENT, refresh);
    window.addEventListener("storage", refresh);
    return () => {
      window.removeEventListener(MACHINE_TAG_COLORS_EVENT, refresh);
      window.removeEventListener("storage", refresh);
    };
  }, []);

  useEffect(() => {
    const refresh = () => setDisplayNames(loadMachineDisplayNames());
    window.addEventListener(MACHINE_DISPLAY_NAMES_EVENT, refresh);
    window.addEventListener("storage", refresh);
    return () => {
      window.removeEventListener(MACHINE_DISPLAY_NAMES_EVENT, refresh);
      window.removeEventListener("storage", refresh);
    };
  }, []);

  const setTagColor = useCallback(
    (source: string, color: SourceColorSlot | null) =>
      setTagColors(setMachineTagColor(source, color)),
    [],
  );
  const setDisplayName = useCallback(
    (source: string, name: string | null) =>
      setDisplayNames(setMachineDisplayName(source, name)),
    [],
  );

  return { tagColors, displayNames, setTagColor, setDisplayName };
}
