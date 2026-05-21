import '@testing-library/jest-dom';

// Provide localStorage stub for jsdom environment (vitest 2.x + jsdom 25).
// jsdom's localStorage may not be fully initialized in all test contexts.
if (typeof localStorage === 'undefined' || !localStorage.getItem) {
    const store: Record<string, string> = {};
    Object.defineProperty(window, 'localStorage', {
        value: {
            getItem: (key: string) => store[key] ?? null,
            setItem: (key: string, value: string) => { store[key] = value; },
            removeItem: (key: string) => { delete store[key]; },
            clear: () => { Object.keys(store).forEach((k) => delete store[k]); },
        },
        writable: true,
    });
}

// Provide matchMedia stub for jsdom environment.
if (!window.matchMedia) {
    Object.defineProperty(window, 'matchMedia', {
        writable: true,
        value: (query: string) => ({
            matches: false,
            media: query,
            onchange: null,
            addListener: () => {},
            removeListener: () => {},
            addEventListener: () => {},
            removeEventListener: () => {},
            dispatchEvent: () => false,
        }),
    });
}
