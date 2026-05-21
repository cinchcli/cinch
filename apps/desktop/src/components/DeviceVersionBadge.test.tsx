import { describe, it, expect, vi } from 'vitest';
import { render, screen, waitFor } from '@testing-library/react';

const getDeviceVersionStatus = vi.fn();
vi.mock('../bindings', () => ({
    commands: {
        getDeviceVersionStatus: (
            reported: string | null,
            clientType: string | null,
            latest: unknown,
        ) => getDeviceVersionStatus(reported, clientType, latest),
    },
}));

import { DeviceVersionBadge } from './DeviceVersionBadge';

describe('DeviceVersionBadge', () => {
    it('renders an em dash when version is missing', () => {
        render(
            <DeviceVersionBadge
                version={null}
                clientType={null}
                latest={{ cli: null, desktop: null, fetched_at: null }}
            />,
        );
        expect(screen.getByText('—')).toBeInTheDocument();
    });

    it('renders the version number and an "outdated" status dot for outdated CLI devices', async () => {
        getDeviceVersionStatus.mockResolvedValueOnce('Outdated');
        render(
            <DeviceVersionBadge
                version="0.1.5"
                clientType="cli"
                latest={{ cli: 'v0.1.8', desktop: null, fetched_at: 0 }}
            />,
        );
        expect(screen.getByText('0.1.5')).toBeInTheDocument();
        await waitFor(() => {
            expect(screen.getByLabelText('outdated')).toBeInTheDocument();
        });
    });

    it('marks up-to-date devices with an "up to date" label so screen readers pick it up', async () => {
        getDeviceVersionStatus.mockResolvedValueOnce('UpToDate');
        render(
            <DeviceVersionBadge
                version="0.1.8"
                clientType="cli"
                latest={{ cli: 'v0.1.8', desktop: null, fetched_at: 0 }}
            />,
        );
        await waitFor(() => {
            expect(screen.getByLabelText('up to date')).toBeInTheDocument();
        });
    });

    it('falls back to "unknown" when the comparison cannot decide', async () => {
        getDeviceVersionStatus.mockResolvedValueOnce('Unknown');
        render(
            <DeviceVersionBadge
                version="dev-build"
                clientType="cli"
                latest={{ cli: 'v0.1.8', desktop: null, fetched_at: 0 }}
            />,
        );
        await waitFor(() => {
            expect(screen.getByLabelText('unknown')).toBeInTheDocument();
        });
    });
});
