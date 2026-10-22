import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent, within } from '@testing-library/react';
import { invoke } from '@tauri-apps/api/core';
import App from './App';
import { useAuthState, retryAuth, type AuthState } from './lib/state/auth';
import type { LocalClip } from './bindings';

// Mock the auth module: AuthProvider becomes a pass-through; useAuthState is type-safely mocked.
vi.mock('./lib/state/auth', () => ({
    AuthProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
    useAuthState: vi.fn(),
    signIn: vi.fn(),
    signOut: vi.fn(),
    retryAuth: vi.fn(),
}));

// Mock Tauri APIs that are not available in the jsdom test environment.
vi.mock('@tauri-apps/plugin-notification', () => ({
    isPermissionGranted: vi.fn(() => Promise.resolve(true)),
    requestPermission: vi.fn(() => Promise.resolve('granted')),
    sendNotification: vi.fn(),
}));
vi.mock('@tauri-apps/api/core', () => ({
    invoke: vi.fn((cmd) => {
        if (cmd === 'list_clips' || cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') {
            return Promise.resolve([]);
        }
        if (cmd === 'get_ws_status') return Promise.resolve('connected');
        return Promise.resolve();
    }),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock('@tauri-apps/api/dpi', () => ({
    LogicalSize: vi.fn().mockImplementation((w: number, h: number) => ({ width: w, height: h })),
}));
vi.mock('@tauri-apps/api/window', () => ({
    getCurrentWindow: vi.fn(() => ({
        startDragging: vi.fn(),
        hide: vi.fn(),
        setSize: vi.fn(() => Promise.resolve()),
        listen: vi.fn(() => Promise.resolve(() => {})),
    })),
}));

describe('App', () => {
    beforeEach(() => {
        vi.clearAllMocks();
        Element.prototype.scrollIntoView = vi.fn();
        vi.mocked(invoke).mockImplementation((cmd) => {
            if (cmd === 'list_clips' || cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') {
                return Promise.resolve([]);
            }
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            if (cmd === 'get_action_shortcuts') {
                return Promise.resolve({
                    edit: 'CmdOrCtrl+E',
                    copy: 'Enter',
                    pin: 'CmdOrCtrl+P',
                    send: 'CmdOrCtrl+Enter',
                    save: 'Command+K',
                });
            }
            return Promise.resolve();
        });
    });

    it('opens local history without a relay or a sign-in dialog', async () => {
        vi.mocked(useAuthState).mockReturnValue({ variant: 'LocalOnly' });
        render(<App />);

        expect(await screen.findByTestId('dashboard-root')).toBeInTheDocument();
        expect(await screen.findByTestId('getting-started-card')).toBeInTheDocument();
        expect(screen.getByLabelText('Search clips')).toBeInTheDocument();
        expect(screen.queryByTestId('onboarding-root')).not.toBeInTheDocument();
        expect(screen.queryByText(/Connect to relay/i)).not.toBeInTheDocument();
        expect(invoke).toHaveBeenCalledWith('get_sources');
        expect(invoke).toHaveBeenCalledWith('list_source_apps');
        expect(invoke).not.toHaveBeenCalledWith('list_devices');
        expect(invoke).not.toHaveBeenCalledWith('list_auth_providers', expect.anything());
    });

    it('opens relay setup only when requested, and can return to local history', async () => {
        vi.mocked(useAuthState).mockReturnValue({ variant: 'LocalOnly' });
        render(<App />);

        fireEvent.click(await screen.findByRole('button', { name: 'Connect a relay' }));
        expect(screen.getByTestId('onboarding-root')).toBeInTheDocument();
        expect(screen.getByText('Optional connection')).toBeInTheDocument();
        fireEvent.click(screen.getByRole('button', { name: /Sign in/i }));
        expect(await screen.findByText(/Connect to relay/i)).toBeInTheDocument();
        fireEvent.click(screen.getByRole('button', { name: 'Close' }));
        fireEvent.click(screen.getByRole('button', { name: 'Inbox' }));
        expect(await screen.findByTestId('getting-started-card')).toBeInTheDocument();
    });

    it('does NOT render AddRelayDialog on Authenticated variant', async () => {
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);
        
        await waitFor(() => {
            expect(screen.getByTestId('dashboard-root')).toBeInTheDocument();
        });
        expect(screen.queryByText(/Connect to relay/i)).not.toBeInTheDocument();
    });

    it('keeps local history available during relay authentication', async () => {
        const state: AuthState = {
            variant: 'Authenticating',
            payload: { progress: { kind: 'SigningIn' } },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);
        
        await waitFor(() => {
            expect(screen.getByText(/signing in/i)).toBeInTheDocument();
            expect(screen.getByTestId('dashboard-root')).toBeInTheDocument();
            expect(screen.getByLabelText('Search clips')).toBeInTheDocument();
        });
    });

    it('keeps local history available when the relay is unreachable', async () => {
        const state: AuthState = {
            variant: 'ErrorRecoverable',
            payload: { reason: { kind: 'RelayUnreachable' }, retry_after_ms: 5000 },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);
        
        await waitFor(() => {
            expect(screen.getByText(/relay unreachable/i)).toBeInTheDocument();
        });
        expect(screen.getByRole('button', { name: /retry now/i })).toBeInTheDocument();
        expect(screen.getByTestId('dashboard-root')).toBeInTheDocument();
        expect(screen.getByLabelText('Search clips')).toBeInTheDocument();
    });

    it.each<AuthState>([
        { variant: 'LocalOnly' },
        { variant: 'ErrorRecoverable', payload: { reason: { kind: 'NetworkDown' }, retry_after_ms: null } },
    ])('searches, copies and pins local clips in $variant', async (state) => {
        vi.mocked(useAuthState).mockReturnValue(state);
        let clip: LocalClip = {
            id: 'offline-1', user_id: '', content: 'offline note', content_type: 'text',
            source: 'local', source_app_id: 'com.apple.TextEdit', source_app: 'TextEdit',
            source_url: null, label: '', byte_size: 12, media_path: null,
            created_at: 1_777_614_529, received_at: 1_777_614_529,
            synced: false, sync_state: 'local', is_pinned: false, pin_note: null,
        };
        const baseInvoke = vi.mocked(invoke).getMockImplementation()!;
        vi.mocked(invoke).mockImplementation((cmd, args) => {
            if (cmd === 'list_clips') return Promise.resolve([{ ...clip }]);
            if (cmd === 'list_pinned_clips') return Promise.resolve(clip.is_pinned ? [{ ...clip }] : []);
            if (cmd === 'pin_clip') {
                clip = { ...clip, is_pinned: true };
                return Promise.resolve();
            }
            return baseInvoke(cmd, args);
        });
        render(<App />);
        const search = await screen.findByLabelText('Search clips');
        fireEvent.change(search, { target: { value: 'offline' } });
        await waitFor(() => expect(invoke).toHaveBeenCalledWith('list_clips', { query: 'offline', limit: 500 }));
        fireEvent.click(await screen.findByRole('button', { name: 'offline note' }));
        expect(screen.queryByRole('button', { name: 'Send clip' })).not.toBeInTheDocument();
        fireEvent.keyDown(window, { key: 'Enter', metaKey: true });
        expect(invoke).not.toHaveBeenCalledWith('send_clip', expect.anything());
        fireEvent.keyDown(window, { key: 'Enter' });
        await waitFor(() => expect(invoke).toHaveBeenCalledWith('copy_clip_to_clipboard', { content: 'offline note' }));

        fireEvent.click(await screen.findByRole('button', { name: 'offline note' }));
        fireEvent.click(screen.getByRole('button', { name: /^Pin\s/ }));
        fireEvent.click(screen.getByRole('button', { name: 'Pin' }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith('pin_clip', { id: 'offline-1', note: null }));
        fireEvent.click(screen.getByRole('button', { name: 'Pinned' }));
        expect(await screen.findByText('offline note')).toBeInTheDocument();
        expect(invoke).not.toHaveBeenCalledWith('list_devices');
    });

    it('keeps selection and shows no success notification when local copy fails', async () => {
        vi.mocked(useAuthState).mockReturnValue({ variant: 'LocalOnly' });
        const clip: LocalClip = {
            id: 'failed-copy', user_id: '', content: 'keep this selection', content_type: 'text',
            source: 'local', source_app_id: null, source_app: null, source_url: null,
            label: '', byte_size: 19, media_path: null, created_at: 1_777_614_529,
            received_at: 1_777_614_529, synced: false, sync_state: 'local',
            is_pinned: false, pin_note: null,
        };
        const baseInvoke = vi.mocked(invoke).getMockImplementation()!;
        vi.mocked(invoke).mockImplementation((cmd, args) => {
            if (cmd === 'list_clips') return Promise.resolve([clip]);
            if (cmd === 'copy_clip_to_clipboard') return Promise.reject('clipboard unavailable');
            return baseInvoke(cmd, args);
        });
        render(<App />);
        const row = await screen.findByRole('button', { name: clip.content });
        fireEvent.click(row);
        fireEvent.keyDown(window, { key: 'Enter' });
        expect(await screen.findByText('Copy failed. Try again.')).toBeInTheDocument();
        expect(row).toHaveAttribute('aria-selected', 'true');
        expect(invoke).not.toHaveBeenCalledWith('show_copy_toast', expect.anything());
        expect(invoke).not.toHaveBeenCalledWith('focus_previous_app');
    });

    it('reports local pin failures and closes the dialog without claiming success', async () => {
        vi.mocked(useAuthState).mockReturnValue({
            variant: 'ErrorRecoverable',
            payload: { reason: { kind: 'RelayUnreachable' }, retry_after_ms: null },
        });
        const clip: LocalClip = {
            id: 'pin-failure', user_id: '', content: 'pin this note', content_type: 'text',
            source: 'local', source_app_id: null, source_app: null, source_url: null,
            label: '', byte_size: 13, media_path: null, created_at: 1_777_614_529,
            received_at: 1_777_614_529, synced: false, sync_state: 'local',
            is_pinned: false, pin_note: null,
        };
        const baseInvoke = vi.mocked(invoke).getMockImplementation()!;
        vi.mocked(invoke).mockImplementation((cmd, args) => {
            if (cmd === 'list_clips') return Promise.resolve([clip]);
            if (cmd === 'pin_clip') return Promise.reject('database unavailable');
            return baseInvoke(cmd, args);
        });
        render(<App />);
        fireEvent.click(await screen.findByRole('button', { name: clip.content }));
        fireEvent.click(screen.getByRole('button', { name: /^Pin\s/ }));
        fireEvent.click(screen.getByRole('button', { name: 'Pin' }));
        expect(await screen.findByText('Pin failed. Try again.')).toBeInTheDocument();
        expect(screen.queryByText('Pin clip')).not.toBeInTheDocument();
        expect(screen.queryByText('Pinned locally')).not.toBeInTheDocument();
        expect(screen.getByLabelText('Search clips')).toBeInTheDocument();
    });

    it('keeps the retry action usable after a failed relay retry', async () => {
        vi.mocked(useAuthState).mockReturnValue({
            variant: 'ErrorRecoverable',
            payload: { reason: { kind: 'RelayUnreachable' }, retry_after_ms: null },
        });
        vi.mocked(retryAuth).mockRejectedValueOnce(new Error('offline'));
        render(<App />);
        fireEvent.click(await screen.findByRole('button', { name: 'Retry now' }));
        expect(await screen.findByText('Retry failed. Local history is available.')).toBeInTheDocument();
        expect(screen.getByRole('button', { name: 'Retry now' })).toBeEnabled();
        expect(screen.getByLabelText('Search clips')).toBeInTheDocument();
    });

    it('shows search misses instead of onboarding in local mode', async () => {
        vi.mocked(useAuthState).mockReturnValue({ variant: 'LocalOnly' });
        render(<App />);
        fireEvent.change(await screen.findByLabelText('Search clips'), { target: { value: 'missing' } });
        expect(await screen.findByText('No results for "missing"')).toBeInTheDocument();
        expect(screen.queryByTestId('getting-started-card')).not.toBeInTheDocument();
    });

    it('focuses search when / is pressed outside text input', async () => {
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);

        const input = await screen.findByLabelText('Search clips');
        input.blur();
        fireEvent.keyDown(window, { key: '/' });

        expect(input).toHaveFocus();
    });

    it('clears the search query after copying the selected search result with Enter', async () => {
        const clip: LocalClip = {
            id: 'c1',
            user_id: 'u1',
            content: 'needle clip',
            content_type: 'text',
            source: 'local',
            source_app_id: null,
            source_app: null,
            source_url: null,
            label: '',
            byte_size: 11,
            media_path: null,
            created_at: 1_777_614_529,
            synced: true,
            is_pinned: false,
            pin_note: null,
            received_at: 1_777_614_529,
        };
        vi.mocked(invoke).mockImplementation((cmd) => {
            if (cmd === 'list_clips') return Promise.resolve([clip]);
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') return Promise.resolve([]);
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            return Promise.resolve();
        });
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);

        const input = await screen.findByLabelText('Search clips');
        fireEvent.change(input, { target: { value: 'needle' } });
        const row = await screen.findByRole('button', { name: /needle clip/i });
        fireEvent.click(row);
        fireEvent.keyDown(window, { key: 'Enter' });

        await waitFor(() => expect(input).toHaveValue(''));
    });

    it('moves selection up with Ctrl+K', async () => {
        const clips: LocalClip[] = [
            {
                id: 'c1',
                user_id: 'u1',
                content: 'first clip',
                content_type: 'text',
                source: 'local',
                source_app_id: null,
                source_app: null,
                source_url: null,
                label: '',
                byte_size: 10,
                media_path: null,
                created_at: 1_777_614_529,
                synced: true,
                is_pinned: false,
                pin_note: null,
                received_at: 1_777_614_529,
            },
            {
                id: 'c2',
                user_id: 'u1',
                content: 'second clip',
                content_type: 'text',
                source: 'local',
                source_app_id: null,
                source_app: null,
                source_url: null,
                label: '',
                byte_size: 11,
                media_path: null,
                created_at: 1_777_614_528,
                synced: true,
                is_pinned: false,
                pin_note: null,
                received_at: 1_777_614_528,
            },
        ];
        vi.mocked(invoke).mockImplementation((cmd) => {
            if (cmd === 'list_clips') return Promise.resolve(clips);
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') return Promise.resolve([]);
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            return Promise.resolve();
        });
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);

        const firstRow = await screen.findByRole('button', { name: /first clip/i });
        const secondRow = await screen.findByRole('button', { name: /second clip/i });
        fireEvent.click(secondRow);

        expect(secondRow).toHaveAttribute('aria-selected', 'true');
        fireEvent.keyDown(window, { key: 'k', ctrlKey: true, code: 'KeyK' });

        await waitFor(() => {
            expect(firstRow).toHaveAttribute('aria-selected', 'true');
        });
    });

    it('copies the edited clip (not the original) when Enter is pressed right after editing', async () => {
        const original: LocalClip = {
            id: 'orig', user_id: 'u1', content: 'ORIGINAL', content_type: 'text',
            source: 'local', source_app_id: null, source_app: null, source_url: null,
            label: '', byte_size: 8, media_path: null, created_at: 1_777_614_500,
            synced: true, is_pinned: false, pin_note: null, received_at: 1_777_614_500,
        };
        const edited: LocalClip = {
            ...original, id: 'edit', content: 'EDITED', byte_size: 6,
            created_at: 1_777_614_999, received_at: 1_777_614_999,
        };
        let editDone = false;
        const copyContents: string[] = [];
        vi.mocked(invoke).mockImplementation((cmd, args?: Record<string, unknown>) => {
            // Post-edit refresh is delayed to mimic real Rust IPC latency: the
            // modal closes (editDialog=null) BEFORE selectedClip is updated to the
            // edited clip, which is the window the focus-restoration races in.
            if (cmd === 'list_clips') {
                if (!editDone) return Promise.resolve([original]);
                return new Promise((r) => setTimeout(() => r([edited, original]), 30));
            }
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') return Promise.resolve([]);
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            if (cmd === 'edit_clip') { editDone = true; return Promise.resolve(edited); }
            if (cmd === 'copy_clip_to_clipboard') { copyContents.push(String(args?.content)); return Promise.resolve(); }
            return Promise.resolve();
        });
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);

        // Select the original clip, then open the edit modal via its Edit button.
        fireEvent.click(await screen.findByRole('button', { name: /ORIGINAL/i }));
        fireEvent.click(screen.getByRole('button', { name: /^Edit/i }));

        // Save & Copy (the mocked edit_clip returns the edited clip regardless of
        // the textarea contents — we are testing the copy target, not the edit).
        fireEvent.click(screen.getByRole('button', { name: /save/i }));

        // The edited clip appears at the top and becomes the selection/preview.
        const editedRow = await screen.findByRole('button', { name: /EDITED/i });
        await waitFor(() => expect(editedRow).toHaveAttribute('aria-selected', 'true'));

        // After the edit modal closes, keyboard focus must land on the EDITED
        // clip's row — not the original. The bug: handleEdit closed the modal
        // before selectedClip was updated, so the focus-restoration effect
        // (App.tsx) targeted the stale (original) row, leaving the keyboard
        // context on the pre-edit clip.
        const active = document.activeElement as HTMLElement | null;
        const activeId = active?.getAttribute?.('data-id');
        expect(activeId).toBe('edit');

        // And pressing Enter copies the edited clip's content.
        fireEvent.keyDown(active ?? window, { key: 'Enter' });
        await waitFor(() => expect(copyContents.length).toBeGreaterThan(0));
        expect(copyContents.at(-1)).toBe('EDITED');
    });

    it('copies an image clip via copy_image_to_clipboard (no media_path) instead of copy_clip_to_clipboard', async () => {
        const clip: LocalClip = {
            id: 'cimg',
            user_id: 'u1',
            content: '',
            content_type: 'image',
            source: 'local',
            source_app_id: null,
            source_app: null,
            source_url: null,
            label: 'screenshot.png',
            byte_size: 245760,
            media_path: null,
            created_at: 1_777_614_529,
            synced: true,
            is_pinned: false,
            pin_note: null,
            received_at: 1_777_614_529,
        };
        vi.mocked(invoke).mockImplementation((cmd) => {
            if (cmd === 'list_clips') return Promise.resolve([clip]);
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') return Promise.resolve([]);
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            return Promise.resolve();
        });
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);

        const row = await screen.findByRole('button', { name: /Image \(240\.0 KB\)/i });
        fireEvent.click(row);
        fireEvent.keyDown(window, { key: 'Enter' });

        await waitFor(() => {
            expect(invoke).toHaveBeenCalledWith('copy_image_to_clipboard', { clipId: 'cimg' });
        });
        expect(invoke).not.toHaveBeenCalledWith('copy_clip_to_clipboard', { content: '' });
    });

    it('moves a copied history clip to the top of the inbox list', async () => {
        const nowSpy = vi.spyOn(Date, 'now').mockReturnValue(1_777_614_600_000);
        const makeClip = (id: string, content: string, createdAt: number): LocalClip => ({
            id,
            user_id: 'u1',
            content,
            content_type: 'text',
            source: 'local',
            source_app_id: null,
            source_app: null,
            source_url: null,
            label: '',
            byte_size: content.length,
            media_path: null,
            created_at: createdAt,
            synced: true,
            sync_state: 'synced',
            is_pinned: false,
            pin_note: null,
            received_at: createdAt,
        });
        const clips = [
            makeClip('new', 'new clip', 1_777_614_529),
            makeClip('old', 'old clip', 1_777_614_100),
        ];
        vi.mocked(invoke).mockImplementation((cmd) => {
            if (cmd === 'list_clips') return Promise.resolve(clips);
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') {
                return Promise.resolve([]);
            }
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            return Promise.resolve();
        });
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        const { container } = render(<App />);

        await screen.findByRole('button', { name: /new clip/i });
        const initialOrder = Array.from(container.querySelectorAll('.clip-row'))
            .map((row) => row.getAttribute('data-id'));
        expect(initialOrder).toEqual(['new', 'old']);

        fireEvent.click(screen.getByRole('button', { name: /old clip/i }));
        fireEvent.keyDown(window, { key: 'Enter' });

        await waitFor(() => {
            const order = Array.from(container.querySelectorAll('.clip-row'))
                .map((row) => row.getAttribute('data-id'));
            expect(order[0]).toBe('old');
        });

        nowSpy.mockRestore();
    });

    it('saves an image clip to file via save_image_to_file when Save... is clicked', async () => {
        const imageClip: LocalClip = {
            id: 'cimg',
            user_id: 'u1',
            content: '',
            content_type: 'image',
            source: 'local',
            source_app_id: null,
            source_app: null,
            source_url: null,
            label: 'screenshot.png',
            byte_size: 245760,
            media_path: null,
            created_at: 1_777_614_529,
            synced: true,
            is_pinned: false,
            pin_note: null,
            received_at: 1_777_614_529,
        };
        vi.mocked(invoke).mockImplementation((cmd) => {
            if (cmd === 'list_clips') return Promise.resolve([imageClip]);
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') {
                return Promise.resolve([]);
            }
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            if (cmd === 'save_image_to_file') return Promise.resolve('/tmp/cinch-20260523-153045.png');
            if (cmd === 'get_action_shortcuts') {
                return Promise.resolve({
                    edit: 'CmdOrCtrl+E',
                    copy: 'Enter',
                    pin: 'CmdOrCtrl+P',
                    send: 'CmdOrCtrl+Enter',
                    save: 'Command+K',
                });
            }
            return Promise.resolve();
        });
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);

        // Click the clip card to select it (same pattern as the copy-image test)
        const row = await screen.findByRole('button', { name: /Image \(240\.0 KB\)/i });
        fireEvent.click(row);

        // The Save... button is rendered by ClipDetail when the clip is selected
        const saveBtn = await screen.findByRole('button', { name: /^save/i });
        fireEvent.click(saveBtn);

        await waitFor(() => {
            expect(invoke).toHaveBeenCalledWith('save_image_to_file', { clipId: 'cimg' });
        });
    });

    it('saves an image clip to file when Command+K is pressed', async () => {
        const imageClip: LocalClip = {
            id: 'cimg',
            user_id: 'u1',
            content: '',
            content_type: 'image',
            source: 'local',
            source_app_id: null,
            source_app: null,
            source_url: null,
            label: 'screenshot.png',
            byte_size: 245760,
            media_path: null,
            created_at: 1_777_614_529,
            synced: true,
            is_pinned: false,
            pin_note: null,
            received_at: 1_777_614_529,
        };
        vi.mocked(invoke).mockImplementation((cmd) => {
            if (cmd === 'list_clips') return Promise.resolve([imageClip]);
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') {
                return Promise.resolve([]);
            }
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            if (cmd === 'save_image_to_file') return Promise.resolve('/tmp/cinch-20260523-153045.png');
            if (cmd === 'get_action_shortcuts') {
                return Promise.resolve({
                    edit: 'CmdOrCtrl+E',
                    copy: 'Enter',
                    pin: 'CmdOrCtrl+P',
                    send: 'CmdOrCtrl+Enter',
                    save: 'Command+K',
                });
            }
            return Promise.resolve();
        });
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);

        const row = await screen.findByRole('button', { name: /Image \(240\.0 KB\)/i });
        fireEvent.click(row);

        fireEvent.keyDown(window, { code: 'KeyK', key: 'k', metaKey: true });

        await waitFor(() => {
            expect(invoke).toHaveBeenCalledWith('save_image_to_file', { clipId: 'cimg' });
        });
    });

    it('does not save a text clip when Command+K is pressed', async () => {
        const textClip: LocalClip = {
            id: 'c1',
            user_id: 'u1',
            content: 'clip text',
            content_type: 'text',
            source: 'local',
            source_app_id: null,
            source_app: null,
            source_url: null,
            label: '',
            byte_size: 9,
            media_path: null,
            created_at: 1_777_614_529,
            synced: true,
            is_pinned: false,
            pin_note: null,
            received_at: 1_777_614_529,
        };
        vi.mocked(invoke).mockImplementation((cmd) => {
            if (cmd === 'list_clips') return Promise.resolve([textClip]);
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') {
                return Promise.resolve([]);
            }
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            if (cmd === 'get_action_shortcuts') {
                return Promise.resolve({
                    edit: 'CmdOrCtrl+E',
                    copy: 'Enter',
                    pin: 'CmdOrCtrl+P',
                    send: 'CmdOrCtrl+Enter',
                    save: 'Command+K',
                });
            }
            return Promise.resolve();
        });
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);

        const row = await screen.findByRole('button', { name: /clip text/i });
        fireEvent.click(row);

        fireEvent.keyDown(window, { code: 'KeyK', key: 'k', metaKey: true });

        await waitFor(() => {
            expect(invoke).not.toHaveBeenCalledWith('save_image_to_file', expect.anything());
        });
    });

    it('does not copy and hide the window when confirming a pin note with Enter', async () => {
        const clip: LocalClip = {
            id: 'c1',
            user_id: 'u1',
            content: 'clip to pin',
            content_type: 'text',
            source: 'local',
            source_app_id: null,
            source_app: null,
            source_url: null,
            label: '',
            byte_size: 11,
            media_path: null,
            created_at: 1_777_614_529,
            synced: true,
            is_pinned: false,
            pin_note: null,
            received_at: 1_777_614_529,
        };
        vi.mocked(invoke).mockImplementation((cmd) => {
            if (cmd === 'list_clips') return Promise.resolve([clip]);
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') return Promise.resolve([]);
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            return Promise.resolve();
        });
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);

        const row = await screen.findByRole('button', { name: /clip to pin/i });
        fireEvent.click(row);
        fireEvent.keyDown(window, { key: 'p', metaKey: true });

        const note = await screen.findByPlaceholderText('Add a note (optional)');
        fireEvent.change(note, { target: { value: 'important' } });
        fireEvent.keyDown(note, { key: 'Enter' });

        await waitFor(() => {
            expect(invoke).toHaveBeenCalledWith('pin_clip', { id: 'c1', note: 'important' });
        });
        expect(invoke).not.toHaveBeenCalledWith('copy_clip_to_clipboard', { content: 'clip to pin' });
        expect(invoke).not.toHaveBeenCalledWith('focus_previous_app');
    });

    it('keeps the clip selected and returns focus to its row after closing the pin modal with Esc', async () => {
        const clip: LocalClip = {
            id: 'c1', user_id: 'u1', content: 'focus return clip', content_type: 'text',
            source: 'local', source_app_id: null, source_app: null, source_url: null,
            label: '', byte_size: 17, media_path: null, created_at: 1_777_614_529,
            synced: true, is_pinned: false, pin_note: null, received_at: 1_777_614_529,
        };
        vi.mocked(invoke).mockImplementation((cmd) => {
            if (cmd === 'list_clips') return Promise.resolve([clip]);
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') return Promise.resolve([]);
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            return Promise.resolve();
        });
        vi.mocked(useAuthState).mockReturnValue({
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        } as AuthState);
        render(<App />);

        const row = await screen.findByRole('button', { name: /focus return clip/i });
        fireEvent.click(row);
        fireEvent.keyDown(window, { key: 'p', metaKey: true });

        const note = await screen.findByPlaceholderText('Add a note (optional)');
        fireEvent.keyDown(note, { key: 'Escape' });

        await waitFor(() => expect(screen.queryByPlaceholderText('Add a note (optional)')).toBeNull());
        // Clip stays selected (detail panel still shows it, not the placeholder)
        // and focus returns to the row instead of dropping to <body>.
        expect(screen.queryByText('Select a clip')).toBeNull();
        expect(document.activeElement).toBe(row);
    });

    it('confirms before deleting the selected clip with ⌘⌫, then deletes on confirm', async () => {
        const clip: LocalClip = {
            id: 'c1',
            user_id: 'u1',
            content: 'clip to delete',
            content_type: 'text',
            source: 'local',
            source_app_id: null,
            source_app: null,
            source_url: null,
            label: '',
            byte_size: 14,
            media_path: null,
            created_at: 1_777_614_529,
            synced: true,
            is_pinned: false,
            pin_note: null,
            received_at: 1_777_614_529,
        };
        vi.mocked(invoke).mockImplementation((cmd) => {
            if (cmd === 'list_clips') return Promise.resolve([clip]);
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') return Promise.resolve([]);
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            return Promise.resolve();
        });
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);

        const row = await screen.findByRole('button', { name: /clip to delete/i });
        fireEvent.click(row);
        fireEvent.keyDown(window, { key: 'Backspace', metaKey: true });

        // Confirmation dialog appears; nothing is deleted until the user confirms.
        const dialog = await screen.findByRole('dialog');
        expect(invoke).not.toHaveBeenCalledWith('delete_clip', { id: 'c1' });

        fireEvent.click(within(dialog).getByRole('button', { name: 'Delete' }));

        await waitFor(() => {
            expect(invoke).toHaveBeenCalledWith('delete_clip', { id: 'c1' });
        });
    });

    it('does not delete when the ⌘⌫ confirmation is cancelled with Esc', async () => {
        const clip: LocalClip = {
            id: 'c1',
            user_id: 'u1',
            content: 'keep me',
            content_type: 'text',
            source: 'local',
            source_app_id: null,
            source_app: null,
            source_url: null,
            label: '',
            byte_size: 7,
            media_path: null,
            created_at: 1_777_614_529,
            synced: true,
            is_pinned: false,
            pin_note: null,
            received_at: 1_777_614_529,
        };
        vi.mocked(invoke).mockImplementation((cmd) => {
            if (cmd === 'list_clips') return Promise.resolve([clip]);
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices' || cmd === 'list_source_apps') return Promise.resolve([]);
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            return Promise.resolve();
        });
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);

        const row = await screen.findByRole('button', { name: /keep me/i });
        fireEvent.click(row);
        fireEvent.keyDown(window, { key: 'Backspace', metaKey: true });

        await screen.findByRole('dialog');
        fireEvent.keyDown(window, { key: 'Escape' });

        await waitFor(() => {
            expect(screen.queryByRole('dialog')).toBeNull();
        });
        expect(invoke).not.toHaveBeenCalledWith('delete_clip', { id: 'c1' });
    });

    it('renders GettingStartedCard when authenticated, inbox empty, and only self device', async () => {
        // localStorage may be sticky from a previous test — ensure the card isn't pre-dismissed.
        localStorage.removeItem('cinchGettingStartedDismissed');
        const state: AuthState = {
            variant: 'Authenticated',
            payload: { user_id: 'u1', device_id: 'd1', hostname: 'h', relay_url: 'http://localhost:8080', active_relay_id: 'r1', machine_id: 'm1' },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);

        await waitFor(() => {
            expect(screen.getByTestId('getting-started-card')).toBeInTheDocument();
        });
    });
});
