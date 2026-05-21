import { describe, it, expect, beforeEach, vi } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { UpdateBanner } from './UpdateBanner';

// Mock the bindings module — we don't want to call into Tauri during tests.
vi.mock('../bindings', () => ({
    commands: {
        getDeviceVersionStatus: vi.fn(),
        runSelfUpdate: vi.fn(),
    },
}));

import { commands } from '../bindings';

const CURRENT_DESKTOP = '0.1.10';

describe('UpdateBanner', () => {
    beforeEach(() => {
        localStorage.clear();
        vi.clearAllMocks();
    });

    it('renders nothing when latest.desktop is null', () => {
        const { container } = render(
            <UpdateBanner
                currentVersion={CURRENT_DESKTOP}
                latest={{ cli: null, desktop: null, fetched_at: null }}
            />,
        );
        expect(container.firstChild).toBeNull();
    });

    it('renders nothing when status is UpToDate', async () => {
        (commands.getDeviceVersionStatus as ReturnType<typeof vi.fn>).mockResolvedValue('UpToDate');
        const { container } = render(
            <UpdateBanner
                currentVersion={CURRENT_DESKTOP}
                latest={{ cli: null, desktop: '0.1.10', fetched_at: 1715000000 }}
            />,
        );
        // Async status fetch settles → banner stays absent.
        await waitFor(() => expect(commands.getDeviceVersionStatus).toHaveBeenCalled());
        expect(container.firstChild).toBeNull();
    });

    it('renders the banner when status is Outdated', async () => {
        (commands.getDeviceVersionStatus as ReturnType<typeof vi.fn>).mockResolvedValue('Outdated');
        render(
            <UpdateBanner
                currentVersion={CURRENT_DESKTOP}
                latest={{ cli: null, desktop: '0.1.11', fetched_at: 1715000000 }}
            />,
        );
        await waitFor(() =>
            expect(screen.getByText(/Cinch 0\.1\.11 is available/i)).toBeInTheDocument(),
        );
        expect(screen.getByRole('button', { name: /install & restart/i })).toBeInTheDocument();
    });

    it('hides the banner when the snoozed version matches latest.desktop', async () => {
        (commands.getDeviceVersionStatus as ReturnType<typeof vi.fn>).mockResolvedValue('Outdated');
        localStorage.setItem('update-snoozed-version', '0.1.11');
        const { container } = render(
            <UpdateBanner
                currentVersion={CURRENT_DESKTOP}
                latest={{ cli: null, desktop: '0.1.11', fetched_at: 1715000000 }}
            />,
        );
        await waitFor(() => expect(commands.getDeviceVersionStatus).toHaveBeenCalled());
        expect(container.firstChild).toBeNull();
    });

    it('reappears when latest.desktop advances past the snoozed version', async () => {
        (commands.getDeviceVersionStatus as ReturnType<typeof vi.fn>).mockResolvedValue('Outdated');
        localStorage.setItem('update-snoozed-version', '0.1.11');
        render(
            <UpdateBanner
                currentVersion={CURRENT_DESKTOP}
                latest={{ cli: null, desktop: '0.1.12', fetched_at: 1715000000 }}
            />,
        );
        await waitFor(() =>
            expect(screen.getByText(/Cinch 0\.1\.12 is available/i)).toBeInTheDocument(),
        );
    });

    it('clicking Install & Restart calls runSelfUpdate', async () => {
        (commands.getDeviceVersionStatus as ReturnType<typeof vi.fn>).mockResolvedValue('Outdated');
        (commands.runSelfUpdate as ReturnType<typeof vi.fn>).mockResolvedValue({ status: 'ok' });
        render(
            <UpdateBanner
                currentVersion={CURRENT_DESKTOP}
                latest={{ cli: null, desktop: '0.1.11', fetched_at: 1715000000 }}
            />,
        );
        const btn = await screen.findByRole('button', { name: /install & restart/i });
        fireEvent.click(btn);
        await waitFor(() => expect(commands.runSelfUpdate).toHaveBeenCalledTimes(1));
    });

    it('clicking dismiss snoozes the current latest.desktop version', async () => {
        (commands.getDeviceVersionStatus as ReturnType<typeof vi.fn>).mockResolvedValue('Outdated');
        const { container } = render(
            <UpdateBanner
                currentVersion={CURRENT_DESKTOP}
                latest={{ cli: null, desktop: '0.1.11', fetched_at: 1715000000 }}
            />,
        );
        const dismiss = await screen.findByRole('button', { name: /dismiss update banner/i });
        fireEvent.click(dismiss);
        expect(localStorage.getItem('update-snoozed-version')).toBe('0.1.11');
        await waitFor(() => expect(container.firstChild).toBeNull());
    });
});
