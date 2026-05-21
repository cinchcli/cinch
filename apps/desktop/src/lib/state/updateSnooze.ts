// useUpdateSnooze — tracks the version the user dismissed the update banner for.
//
// localStorage doesn't natively notify same-tab listeners on setItem, so we
// wrap writes through `setSnoozedVersion` and emit a custom event the hook
// subscribes to. Same-tab dismissals trigger a re-render; cross-tab writes
// flow through the native `storage` event.

import { useEffect, useState } from 'react';

const KEY = 'update-snoozed-version';
const EVENT = 'cinch:update-snooze-changed';

export function getSnoozedVersion(): string | null {
    try {
        return localStorage.getItem(KEY);
    } catch {
        return null;
    }
}

export function setSnoozedVersion(version: string | null): void {
    try {
        if (version === null) {
            localStorage.removeItem(KEY);
        } else {
            localStorage.setItem(KEY, version);
        }
        window.dispatchEvent(new CustomEvent(EVENT, { detail: version }));
    } catch {
        // localStorage may be unavailable (private mode, quota). Silent fallback —
        // the banner will keep showing until the user upgrades.
    }
}

export function useUpdateSnooze(): { snoozed: string | null; setSnoozed: (v: string | null) => void } {
    const [snoozed, setLocal] = useState<string | null>(getSnoozedVersion());

    useEffect(() => {
        const onChange = (e: Event) => {
            if (e instanceof CustomEvent) {
                setLocal(typeof e.detail === 'string' ? e.detail : null);
            } else {
                setLocal(getSnoozedVersion());
            }
        };
        window.addEventListener(EVENT, onChange);
        window.addEventListener('storage', onChange);
        return () => {
            window.removeEventListener(EVENT, onChange);
            window.removeEventListener('storage', onChange);
        };
    }, []);

    return { snoozed, setSnoozed: setSnoozedVersion };
}
