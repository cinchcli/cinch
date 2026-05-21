// useLatestVersions — subscribes to the desktop's GitHub-release cache.
//
// On mount the hook reads the cached `LatestVersions` (which also fires
// a background refresh in `get_latest_versions` when stale), then keeps
// `latest` in sync with `latestVersionsUpdated` events emitted from the
// 6-hour refresh loop on the Rust side. The returned object is wire-
// identical to `commands.getDeviceVersionStatus`'s `latest` argument, so
// it can be passed straight through.

import { useEffect, useState } from 'react';
import { commands, events, type LatestVersions } from '../../bindings';

const EMPTY: LatestVersions = { cli: null, desktop: null, fetched_at: null };

export function useLatestVersions(): LatestVersions {
    const [latest, setLatest] = useState<LatestVersions>(EMPTY);

    useEffect(() => {
        let mounted = true;
        commands.getLatestVersions().then((v) => {
            if (mounted && v != null) setLatest(v);
        });
        const unsubPromise = events.latestVersionsUpdated.listen((e) => {
            if (mounted) setLatest(e.payload);
        });
        return () => {
            mounted = false;
            unsubPromise.then((fn) => fn());
        };
    }, []);

    return latest;
}
