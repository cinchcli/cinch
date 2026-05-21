import { describe, it, expect, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';

const { runSelfUpdate } = vi.hoisted(() => ({
    runSelfUpdate: vi.fn(async () => undefined),
}));
vi.mock('../bindings', () => ({
    commands: { runSelfUpdate },
}));

import { DeviceUpdateAction } from './DeviceUpdateAction';

describe('DeviceUpdateAction', () => {
    it('renders nothing when the device is up to date', () => {
        const { container } = render(
            <DeviceUpdateAction status="UpToDate" isOwnDesktop clientType="desktop" />,
        );
        expect(container).toBeEmptyDOMElement();
    });

    it('renders nothing when status is unknown', () => {
        const { container } = render(
            <DeviceUpdateAction status="Unknown" isOwnDesktop clientType="desktop" />,
        );
        expect(container).toBeEmptyDOMElement();
    });

    it('renders an Update button for the user’s own outdated desktop and triggers self-update', () => {
        runSelfUpdate.mockClear();
        render(
            <DeviceUpdateAction status="Outdated" isOwnDesktop clientType="desktop" />,
        );
        const btn = screen.getByRole('button', { name: /update/i });
        fireEvent.click(btn);
        expect(runSelfUpdate).toHaveBeenCalledTimes(1);
    });

    it('renders a "How to update" link for outdated CLI peers', () => {
        render(
            <DeviceUpdateAction
                status="Outdated"
                isOwnDesktop={false}
                clientType="cli"
            />,
        );
        const link = screen.getByRole('link', { name: /how to update/i });
        expect(link).toHaveAttribute(
            'href',
            'https://cinchcli.com/docs/update#cli',
        );
    });

    it('renders a docs link for outdated remote desktops too', () => {
        render(
            <DeviceUpdateAction
                status="Outdated"
                isOwnDesktop={false}
                clientType="desktop"
            />,
        );
        const link = screen.getByRole('link', { name: /how to update/i });
        expect(link).toHaveAttribute(
            'href',
            'https://cinchcli.com/docs/update#desktop',
        );
    });
});
