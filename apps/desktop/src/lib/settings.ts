import { useEffect, useState } from 'react';

const NOTIFY_KEY = 'cinch.notify_on_remote_login';

export function getNotifyOnRemoteLogin(): boolean {
    const raw = localStorage.getItem(NOTIFY_KEY);
    if (raw === null) return true; // default: enabled
    return raw === '1';
}

export function setNotifyOnRemoteLogin(value: boolean): void {
    localStorage.setItem(NOTIFY_KEY, value ? '1' : '0');
    // Manually dispatch a storage event so same-window consumers re-render.
    // (Native `storage` events only fire across windows, not within one.)
    window.dispatchEvent(new StorageEvent('storage', { key: NOTIFY_KEY, newValue: value ? '1' : '0' }));
}

export function useNotifyOnRemoteLogin(): [boolean, (v: boolean) => void] {
    const [value, setValue] = useState(getNotifyOnRemoteLogin);
    useEffect(() => {
        const handler = (e: StorageEvent) => {
            if (e.key === NOTIFY_KEY) {
                setValue(e.newValue === '1');
            }
        };
        window.addEventListener('storage', handler);
        return () => window.removeEventListener('storage', handler);
    }, []);
    return [
        value,
        (v: boolean) => {
            setValue(v);
            setNotifyOnRemoteLogin(v);
        },
    ];
}
