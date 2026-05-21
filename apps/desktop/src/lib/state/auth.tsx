// desktop/src/state/auth.tsx — AuthState discriminated union + AuthProvider + useAuthState hook.
// This is the implementation file; auth.ts re-exports from here to satisfy SC#4's named path.
//
// Mirrors desktop/src-tauri/src/auth/state.rs::AuthState exactly.
// Rust serializes with #[serde(tag = "variant", content = "payload")],
// so JSON looks like: { "variant": "Authenticated", "payload": { user_id, device_id, ... } }.

import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { commands, events } from '../../bindings';
import { unwrap } from '../tauri';
import { onOpenUrl, getCurrent } from '@tauri-apps/plugin-deep-link';

export type AuthProgress =
    | { kind: 'SigningIn' }
    | { kind: 'Pairing' }
    | { kind: 'RotatingToken' };

export type AuthErrorReason =
    | { kind: 'RelayUnreachable' }
    | { kind: 'KeyringUnavailable' }
    | { kind: 'NetworkDown' }
    | { kind: 'InvalidPairToken' };

type AuthenticatedPayload = {
    user_id: string;
    device_id: string;
    hostname: string;
    relay_url: string;
    active_relay_id: string;
    machine_id: string;
};

type ErrorRecoverablePayload = {
    reason: AuthErrorReason;
    retry_after_ms: number | null;
};

export type AuthState =
    | { variant: 'LocalOnly' }
    | { variant: 'Authenticating'; payload: { progress: AuthProgress } }
    | { variant: 'Authenticated'; payload: AuthenticatedPayload }
    | { variant: 'ErrorRecoverable'; payload: ErrorRecoverablePayload };

// LocalOnly is the only safe default before the first get_auth_state resolves.
// This matches the Rust side's `AuthState::default() = LocalOnly`.
const DEFAULT_STATE: AuthState = { variant: 'LocalOnly' };

const AuthContext = createContext<AuthState>(DEFAULT_STATE);

/** Process deep-link URLs received via getCurrent() or onOpenUrl(). */
async function handleDeepLinkUrls(urls: string[]): Promise<void> {
    for (const urlStr of urls) {
        try {
            const url = new URL(urlStr);
            // cinch://auth/callback?token=X&device_id=Y&user_id=Z&relay_url=R
            const isAuth = url.host === 'auth' || url.pathname === '/auth/callback';
            if (!isAuth) continue;

            const token = url.searchParams.get('token');
            const deviceId = url.searchParams.get('device_id');
            const userId = url.searchParams.get('user_id');

            if (token && deviceId && userId) {
                // Invoke Rust-side handler for credential write + state transition
                await unwrap(commands.handleDeeplink(urlStr));
            }
        } catch (e) {
            console.warn('deep-link parse error:', e);
        }
    }
}

export function AuthProvider({ children }: { children: ReactNode }) {
    const [state, setState] = useState<AuthState>(DEFAULT_STATE);

    useEffect(() => {
        // Initial snapshot from Rust.
        commands.getAuthState()
            .then((s) => setState(s))
            .catch((err) => {
                console.warn('get_auth_state failed:', err);
            });

        // Subscribe to subsequent transitions.
        const unsub = events.authStateChanged.listen((event) => {
            setState(event.payload);
        });

        // Cold-start: check if app was opened via deep-link
        getCurrent()
            .then((urls) => {
                if (urls && urls.length > 0) handleDeepLinkUrls(urls);
            })
            .catch(() => {
                // getCurrent may fail if not launched via deep-link — safe to ignore
            });

        // Hot-app: listen for deep-link events while running
        const unsubDeepLink = onOpenUrl((urls) => {
            handleDeepLinkUrls(urls);
        });

        return () => {
            unsub.then((f) => f());
            unsubDeepLink.then((f) => f());
        };
    }, []);

    return <AuthContext.Provider value={state}>{children}</AuthContext.Provider>;
}

export function useAuthState(): AuthState {
    return useContext(AuthContext);
}

// Imperative actions — thin wrappers over typed commands.
// React components call these; Rust owns all state transitions.
export async function signIn(relay_url: string, provider?: string): Promise<void> {
    await unwrap(commands.signIn(relay_url, provider ?? null));
}

export async function signOut(): Promise<void> {
    await unwrap(commands.signOut());
}

export async function retryAuth(): Promise<void> {
    await unwrap(commands.retryAuth());
}
