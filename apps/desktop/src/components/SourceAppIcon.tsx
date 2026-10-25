import { useEffect, useState, type CSSProperties, type ReactNode } from 'react';
import { commands } from '../bindings';

// One IPC lookup per bundle id for the lifetime of the window. `null` means
// the app is not installed (or the lookup failed) and the fallback is shown.
const pending = new Map<string, Promise<string | null>>();
const resolved = new Map<string, string | null>();

function loadAppIcon(appId: string): Promise<string | null> {
  let icon = pending.get(appId);
  if (!icon) {
    icon = commands.getAppIcon(appId)
      .catch(() => null)
      .then((src) => {
        resolved.set(appId, src);
        return src;
      });
    pending.set(appId, icon);
  }
  return icon;
}

interface SourceAppIconProps {
  appId: string;
  size: number;
  fallback?: ReactNode;
  testId?: string;
  style?: CSSProperties;
}

export function SourceAppIcon({ appId, size, fallback = null, testId, style }: SourceAppIconProps) {
  const [loaded, setLoaded] = useState<{ appId: string; src: string | null } | null>(null);

  useEffect(() => {
    if (resolved.has(appId)) return;
    let active = true;
    void loadAppIcon(appId).then((src) => {
      if (active) setLoaded({ appId, src });
    });
    return () => { active = false; };
  }, [appId]);

  const src = resolved.get(appId) ?? (loaded?.appId === appId ? loaded.src : null);
  if (!src) return <>{fallback}</>;
  return <img data-testid={testId} src={src} width={size} height={size} alt="" aria-hidden="true" style={style} />;
}
