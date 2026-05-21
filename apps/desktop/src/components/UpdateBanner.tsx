import { useEffect, useState } from 'react';
import { commands, type LatestVersions, type VersionStatus } from '../bindings';
import { useUpdateSnooze } from '../lib/state/updateSnooze';
import { C } from '../design';
import { IconX } from '../icons';

interface Props {
    currentVersion: string;
    latest: LatestVersions;
}

type State =
    | { kind: 'idle' }
    | { kind: 'downloading' }
    | { kind: 'error'; message: string };

export function UpdateBanner({ currentVersion, latest }: Props) {
    const [status, setStatus] = useState<VersionStatus | null>(null);
    const [state, setState] = useState<State>({ kind: 'idle' });
    const { snoozed, setSnoozed } = useUpdateSnooze();

    useEffect(() => {
        if (!latest.desktop) {
            setStatus(null);
            return;
        }
        let cancelled = false;
        commands.getDeviceVersionStatus(currentVersion, 'desktop', latest).then((s) => {
            if (!cancelled) setStatus(s);
        });
        return () => {
            cancelled = true;
        };
    }, [currentVersion, latest]);

    if (!latest.desktop || status !== 'Outdated' || snoozed === latest.desktop) {
        return null;
    }

    const onInstall = async () => {
        setState({ kind: 'downloading' });
        try {
            await commands.runSelfUpdate();
            // If runSelfUpdate succeeds, the app calls app.restart() and this
            // process is gone before we get here. If we ever get here, treat
            // it as a no-op success.
        } catch (e: unknown) {
            const msg = e instanceof Error ? e.message : String(e);
            setState({ kind: 'error', message: msg });
        }
    };

    const onDismiss = () => {
        setSnoozed(latest.desktop);
    };

    return (
        <div
            role="status"
            aria-live="polite"
            style={{
                width: '100%',
                height: 40,
                background: C.card,
                borderBottom: `1px solid ${C.border}`,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'space-between',
                padding: '0 16px',
                flexShrink: 0,
                fontFamily: 'var(--font-body)',
                fontSize: 14,
            }}
        >
            <span style={{ color: C.t2 }}>
                {state.kind === 'error' ? (
                    <>Update failed: {state.message}</>
                ) : (
                    <>Cinch {latest.desktop} is available</>
                )}
            </span>
            <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
                <button
                    type="button"
                    onClick={onInstall}
                    disabled={state.kind === 'downloading'}
                    style={{
                        background: 'transparent',
                        color: C.t1,
                        border: `1px solid ${C.border}`,
                        borderRadius: 'var(--radius-md)',
                        padding: 'var(--sp-xs) var(--sp-sm)',
                        fontSize: 12,
                        fontWeight: 500,
                        cursor: state.kind === 'downloading' ? 'wait' : 'pointer',
                        fontFamily: 'var(--font-body)',
                    }}
                >
                    {state.kind === 'downloading'
                        ? 'Downloading…'
                        : state.kind === 'error'
                          ? 'Retry'
                          : 'Install & Restart'}
                </button>
                <button
                    type="button"
                    aria-label="Dismiss update banner"
                    onClick={onDismiss}
                    style={{
                        background: 'none',
                        border: 'none',
                        cursor: 'pointer',
                        padding: 8,
                        borderRadius: 4,
                        color: C.t3,
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        width: 28,
                        height: 28,
                    }}
                >
                    <IconX size={12} />
                </button>
            </div>
        </div>
    );
}
