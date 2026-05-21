// DeviceVersionCell — one row's version badge + update action.
//
// Lives as its own component because each device row needs its own
// `useEffect` to fetch the comparison status (and React forbids calling
// hooks inside a parent's `map`). We pass the resolved status to
// `DeviceUpdateAction` so it doesn't have to repeat the round-trip.

import { useEffect, useState } from 'react';
import {
    commands,
    type LatestVersions,
    type VersionStatus,
} from '../bindings';
import { DeviceVersionBadge } from './DeviceVersionBadge';
import { DeviceUpdateAction } from './DeviceUpdateAction';

interface Props {
    version: string | null;
    clientType: string | null;
    latest: LatestVersions;
    isOwnDesktop: boolean;
}

export function DeviceVersionCell({
    version,
    clientType,
    latest,
    isOwnDesktop,
}: Props) {
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

    return (
        <span
            style={{
                display: 'inline-flex',
                alignItems: 'center',
                gap: 8,
            }}
        >
            <DeviceVersionBadge
                version={version}
                clientType={clientType}
                latest={latest}
            />
            <DeviceUpdateAction
                status={status}
                isOwnDesktop={isOwnDesktop}
                clientType={clientType}
            />
        </span>
    );
}
