import { describe, it, expect, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useUpdateSnooze, getSnoozedVersion, setSnoozedVersion } from './updateSnooze';

describe('updateSnooze', () => {
    beforeEach(() => {
        localStorage.clear();
    });

    it('reads null when nothing is snoozed', () => {
        expect(getSnoozedVersion()).toBeNull();
    });

    it('round-trips a snoozed version', () => {
        setSnoozedVersion('0.1.11');
        expect(getSnoozedVersion()).toBe('0.1.11');
    });

    it('hook returns the current snoozed value on mount', () => {
        setSnoozedVersion('0.1.11');
        const { result } = renderHook(() => useUpdateSnooze());
        expect(result.current.snoozed).toBe('0.1.11');
    });

    it('hook reactively updates when setSnoozed is called', () => {
        const { result } = renderHook(() => useUpdateSnooze());
        expect(result.current.snoozed).toBeNull();
        act(() => {
            result.current.setSnoozed('0.1.11');
        });
        expect(result.current.snoozed).toBe('0.1.11');
    });

    it('hook reactively updates when external setSnoozedVersion is called', () => {
        const { result } = renderHook(() => useUpdateSnooze());
        act(() => {
            setSnoozedVersion('0.1.12');
        });
        expect(result.current.snoozed).toBe('0.1.12');
    });
});
