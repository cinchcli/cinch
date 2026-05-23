import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { commands } from '../bindings';
import { DevicesPanel } from './DevicesPanel';

vi.mock('../bindings', () => ({
  commands: {
    listDevices: vi.fn(),
    getSources: vi.fn(),
    getAllSourceAlertSettings: vi.fn(),
    setSourceAlertEnabled: vi.fn(),
    setDeviceNickname: vi.fn(),
    revokeDevice: vi.fn(),
    getLatestVersions: vi.fn(async () => ({
      cli: 'v0.1.8',
      desktop: 'v0.1.7',
      fetched_at: 0,
    })),
    getDeviceVersionStatus: vi.fn(async (reported: string | null) => {
      if (reported === '0.1.5') return 'Outdated';
      if (reported === '0.1.8' || reported === '0.1.7') return 'UpToDate';
      return 'Unknown';
    }),
    runSelfUpdate: vi.fn(async () => undefined),
  },
  events: {
    latestVersionsUpdated: {
      listen: vi.fn(() => Promise.resolve(() => {})),
    },
    devicesChanged: {
      listen: vi.fn(() => Promise.resolve(() => {})),
    },
  },
}));

describe('DevicesPanel', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
    vi.mocked(commands.listDevices).mockResolvedValue({
      status: 'ok',
      data: [
        {
          id: 'd-prod',
          hostname: 'prod',
          source_key: 'remote:prod',
          clip_count: 3,
          last_push_at: '2026-05-06T01:00:00Z',
          online: true,
          nickname: 'Prod',
          client_version: '0.1.5',
          client_type: 'cli',
        },
      ],
    });
    vi.mocked(commands.getSources).mockResolvedValue({ status: 'ok', data: [] });
    vi.mocked(commands.getAllSourceAlertSettings).mockResolvedValue({
      status: 'ok',
      data: [{ source: 'remote:prod', alert_enabled: false }],
    });
    vi.mocked(commands.setSourceAlertEnabled).mockResolvedValue({ status: 'ok', data: null });
    vi.mocked(commands.setDeviceNickname).mockResolvedValue({ status: 'ok', data: null });
  });

  it('shows a per-device desktop alert toggle and persists changes', async () => {
    render(
      <DevicesPanel
        currentDeviceID="local-device"
        onShowToast={vi.fn()}
      />,
    );

    const toggle = await screen.findByRole('button', { name: /turn desktop alerts on for prod/i });
    expect(toggle).toHaveTextContent(/alerts off/i);

    fireEvent.click(toggle);

    await waitFor(() => {
      expect(commands.setSourceAlertEnabled).toHaveBeenCalledWith('remote:prod', true);
    });
  });

  it('shows device customization inline and persists a tag color', async () => {
    render(
      <DevicesPanel
        currentDeviceID="local-device"
        onShowToast={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByRole('button', { name: /customize prod/i }));
    const settings = screen.getByRole('region', { name: /device settings for prod/i });
    fireEvent.click(screen.getByRole('button', { name: /amber color for prod/i }));

    expect(localStorage.getItem('cinch.machineTagColors.v1')).toBe(
      JSON.stringify({ 'remote:prod': 'amber' }),
    );
    expect(screen.queryByLabelText(/choose tag color for prod/i)).not.toBeInTheDocument();
    expect(settings).toContainElement(screen.getByRole('button', { name: /amber color for prod/i }));
    expect(screen.getAllByText('Prod')[0]).toHaveStyle({
      background: 'var(--pill-2-bg)',
      color: 'var(--pill-2-fg)',
    });
  });

  it('makes device name editing explicit and persists the new name', async () => {
    render(
      <DevicesPanel
        currentDeviceID="local-device"
        onShowToast={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByRole('button', { name: /customize prod/i }));
    const input = screen.getByLabelText(/device display name/i);
    fireEvent.change(input, { target: { value: 'Build Mac' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    await waitFor(() => {
      expect(commands.setDeviceNickname).toHaveBeenCalledWith('d-prod', 'Build Mac');
    });
  });

  it('shows the device version badge and a "How to update" link for outdated CLI peers', async () => {
    render(
      <DevicesPanel
        currentDeviceID="local-device"
        onShowToast={vi.fn()}
      />,
    );

    expect(await screen.findByText('0.1.5')).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByLabelText('outdated')).toBeInTheDocument();
    });
    const link = screen.getByRole('link', { name: /how to update/i });
    expect(link).toHaveAttribute('href', 'https://cinchcli.com/docs/update#cli');
  });

  it('prefills the Name input with the stored nickname (not the hostname fallback)', async () => {
    render(
      <DevicesPanel
        currentDeviceID="local-device"
        onShowToast={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByRole('button', { name: /customize prod/i }));
    const input = screen.getByLabelText(/device display name/i) as HTMLInputElement;
    expect(input.value).toBe('Prod');
    expect(input.placeholder).toBe('prod');
  });

  it('saves via the Save button and closes the customize panel on success', async () => {
    render(
      <DevicesPanel
        currentDeviceID="local-device"
        onShowToast={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByRole('button', { name: /customize prod/i }));
    const input = screen.getByLabelText(/device display name/i);
    fireEvent.change(input, { target: { value: 'Build Mac' } });
    fireEvent.click(screen.getByRole('button', { name: /save display name for prod/i }));

    await waitFor(() => {
      expect(commands.setDeviceNickname).toHaveBeenCalledWith('d-prod', 'Build Mac');
    });
    await waitFor(() => {
      expect(
        screen.queryByRole('region', { name: /device settings for prod/i }),
      ).not.toBeInTheDocument();
    });
  });

  it('Cancel closes the customize panel without saving', async () => {
    render(
      <DevicesPanel
        currentDeviceID="local-device"
        onShowToast={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByRole('button', { name: /customize prod/i }));
    const input = screen.getByLabelText(/device display name/i);
    fireEvent.change(input, { target: { value: 'Discarded' } });
    fireEvent.click(screen.getByRole('button', { name: /cancel customizing prod/i }));

    expect(commands.setDeviceNickname).not.toHaveBeenCalled();
    expect(
      screen.queryByRole('region', { name: /device settings for prod/i }),
    ).not.toBeInTheDocument();
  });

  it('persists a local display name for source-only devices', async () => {
    vi.mocked(commands.listDevices).mockResolvedValue({ status: 'ok', data: [] });
    vi.mocked(commands.getSources).mockResolvedValue({
      status: 'ok',
      data: [{ source: 'remote:ci-runner', clip_count: 9, last_seen: 1_777_614_529 }],
    });

    render(
      <DevicesPanel
        currentDeviceID="local-device"
        onShowToast={vi.fn()}
      />,
    );

    fireEvent.click(await screen.findByRole('button', { name: /customize ci-runner/i }));
    const input = screen.getByLabelText(/device display name/i);
    fireEvent.change(input, { target: { value: 'CI Runner' } });
    fireEvent.keyDown(input, { key: 'Enter' });

    expect(localStorage.getItem('cinch.machineDisplayNames.v1')).toBe(
      JSON.stringify({ 'remote:ci-runner': 'CI Runner' }),
    );
    expect(await screen.findByText('CI Runner')).toBeInTheDocument();
  });
});
