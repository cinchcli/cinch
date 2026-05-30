import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { invoke } from '@tauri-apps/api/core';
import App from './App';
import { useAuthState, type AuthState } from './lib/state/auth';
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
        if (cmd === 'list_clips' || cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices') {
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
            if (cmd === 'list_clips' || cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices') {
                return Promise.resolve([]);
            }
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            return Promise.resolve();
        });
    });

    it('renders AddRelayDialog on LocalOnly variant', async () => {
        const state: AuthState = { variant: 'LocalOnly' };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);
        
        await waitFor(() => {
            expect(screen.getByText(/Connect to relay/i)).toBeInTheDocument();
        });
        expect(screen.queryByTestId('setup-screen')).not.toBeInTheDocument();
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

    it('renders AuthLoadingScreen on Authenticating variant', async () => {
        const state: AuthState = {
            variant: 'Authenticating',
            payload: { progress: { kind: 'SigningIn' } },
        };
        vi.mocked(useAuthState).mockReturnValue(state);
        render(<App />);
        
        await waitFor(() => {
            expect(screen.getByText(/signing in/i)).toBeInTheDocument();
        });
    });

    it('renders AuthErrorScreen on ErrorRecoverable variant', async () => {
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
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices') return Promise.resolve([]);
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
        expect(invoke).toHaveBeenCalledWith('mark_clip_copied', { id: 'c1' });
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
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices') return Promise.resolve([]);
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
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices') return Promise.resolve([]);
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
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices') {
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
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices') {
                return Promise.resolve([]);
            }
            if (cmd === 'get_ws_status') return Promise.resolve('connected');
            if (cmd === 'save_image_to_file') return Promise.resolve('/tmp/cinch-20260523-153045.png');
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
            if (cmd === 'list_pinned_clips' || cmd === 'get_sources' || cmd === 'list_devices') return Promise.resolve([]);
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
