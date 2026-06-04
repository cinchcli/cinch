import { useEffect, useState } from 'react';
import { commands, type LatestVersions, type VersionStatus } from '../bindings';
import { C } from '../design';

interface Props {
    version: string | null;
    clientType: string | null;
    latest: LatestVersions;
}

const STATUS_LABEL: Record<VersionStatus, string> = {
    UpToDate: 'up to date',
    Outdated: 'outdated',
    Unknown: 'unknown',
};

export function DeviceVersionBadge({ version, clientType, latest }: Props) {
    const [status, setStatus] = useState<VersionStatus>('Unknown');

    useEffect(() => {
        if (!version || !clientType) {
            setStatus('Unknown');
            return;
        }
        let mounted = true;
        commands.getDeviceVersionStatus(version, clientType, latest).then((s) => {
            if (mounted) setStatus(s);
        });
        return () => {
            mounted = false;
        };
    }, [version, clientType, latest]);

    if (!version) {
        return <span style={{ color: C.t3 }}>—</span>;
    }

    // Monochrome redesign: status reads through fill intensity, not hue. Outdated
    // (needs attention) is the most prominent; up-to-date is quiet; unknown faintest.
    // The aria-label and the "How to update" link in DevicesPanel carry the meaning.
    const dotColor =
        status === 'Outdated' ? C.t1 : status === 'UpToDate' ? C.t3 : C.t4;

    return (
        <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <span
                aria-label={STATUS_LABEL[status]}
                role="img"
                style={{
                    width: 8,
                    height: 8,
                    borderRadius: '50%',
                    background: dotColor,
                    flexShrink: 0,
                }}
            />
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12 }}>{version}</span>
        </span>
    );
}
