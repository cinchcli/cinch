import { useState, useEffect, useRef } from "react";
import { commands } from "../bindings";
import { unwrap } from "../lib/tauri";
import { signIn } from "../lib/state/auth";
import { C } from "../design";
import { IconX } from "../icons";

interface AddRelayDialogProps {
  onClose: () => void;
  /// When set, pre-fills the relay URL field and switches the dialog title
  /// to indicate the dialog was opened by a CLI handoff (cinch://login).
  initialRelayUrl?: string;
  fromCli?: boolean;
  hideClose?: boolean;
}

type Method = "browser" | "token";
type Provider = "github" | "google";

// Fetches available OAuth providers from the relay via the Tauri command.
// The Rust side bypasses the WebView's CSP (`connect-src 'self' ipc:`),
// which would otherwise block a cross-origin fetch from the frontend.
// Returns [] on any error (network down, self-host without OAuth, etc.).
async function fetchProviders(relayUrl: string): Promise<Provider[]> {
  try {
    const list = await commands.listAuthProviders(relayUrl);
    return list.filter((p): p is Provider => p === "github" || p === "google");
  } catch {
    return [];
  }
}

export function AddRelayDialog({ onClose, initialRelayUrl, fromCli, hideClose }: AddRelayDialogProps) {
  const [method, setMethod] = useState<Method>("browser");

  // Browser method
  const [browserUrl, setBrowserUrl] = useState(initialRelayUrl ?? "");
  const [browserLoading, setBrowserLoading] = useState(false);
  const [browserError, setBrowserError] = useState<string | null>(null);
  const [providers, setProviders] = useState<Provider[] | null>(null); // null = not yet fetched
  const [providersLoading, setProvidersLoading] = useState(false);
  const fetchTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Token method
  const [tokenUrl, setTokenUrl] = useState("");
  const [token, setToken] = useState("");
  const [tokenLoading, setTokenLoading] = useState(false);
  const [tokenError, setTokenError] = useState<string | null>(null);

  // Debounce relay URL input and fetch providers
  useEffect(() => {
    const url = browserUrl.trim().replace(/\/$/, "");
    if (!url.startsWith("http")) {
      setProviders(null);
      return;
    }
    if (fetchTimer.current) clearTimeout(fetchTimer.current);
    fetchTimer.current = setTimeout(async () => {
      setProvidersLoading(true);
      const result = await fetchProviders(url);
      setProviders(result);
      setProvidersLoading(false);
    }, 600);
    return () => { if (fetchTimer.current) clearTimeout(fetchTimer.current); };
  }, [browserUrl]);

  const handleBrowserConnect = async (provider?: Provider) => {
    const relay = browserUrl.trim().replace(/\/$/, "");
    if (!relay) { setBrowserError("Relay URL is required"); return; }
    setBrowserLoading(true);
    setBrowserError(null);
    try {
      await signIn(relay, provider);
      onClose();
    } catch (e) {
      setBrowserError(String(e));
    } finally {
      setBrowserLoading(false);
    }
  };

  const handleTokenConnect = async () => {
    const relay = tokenUrl.trim().replace(/\/$/, "");
    if (!relay) { setTokenError("Relay URL is required"); return; }
    if (!token.trim()) { setTokenError("Pairing token is required"); return; }
    setTokenLoading(true);
    setTokenError(null);
    try {
      await unwrap(commands.pairWithToken({
        relay_url: relay,
        pair_token: token.trim(),
        label: null,
      }));
      onClose();
    } catch (e) {
      setTokenError(String(e));
    } finally {
      setTokenLoading(false);
    }
  };

  const S = {
    overlay: {
      position: "fixed" as const,
      inset: 0,
      background: "rgba(0,0,0,0.55)",
      zIndex: 300,
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
    },
    pane: {
      background: C.bg,
      color: C.t1,
      width: "100%",
      maxWidth: 440,
      padding: "28px 32px",
      borderRadius: 12,
      border: `1px solid ${C.border}`,
    },
    titleRow: {
      display: "flex",
      alignItems: "center",
      justifyContent: "space-between",
      marginBottom: 20,
    },
    fieldLabel: {
      fontSize: 11,
      fontWeight: 600,
      color: C.t3,
      letterSpacing: "0.01em",
      marginBottom: 4,
    },
    input: (error?: boolean) => ({
      width: "100%",
      background: C.card,
      border: `1px solid ${error ? C.error : C.border}`,
      borderRadius: 6,
      padding: "7px 10px",
      fontSize: 13,
      color: C.t1,
      fontFamily: "inherit",
      outline: "none",
      boxSizing: "border-box" as const,
      marginBottom: 12,
    }),
    connectBtn: (loading?: boolean) => ({
      background: C.accent,
      color: C.accentOn,
      border: "none",
      borderRadius: 6,
      padding: "8px 18px",
      fontSize: 13,
      fontWeight: 600,
      cursor: loading ? "not-allowed" : "pointer",
      opacity: loading ? 0.6 : 1,
      marginTop: 4,
      width: "100%",
      fontFamily: "inherit",
    }),
    providerBtn: (loading?: boolean, bg?: string, textColor?: string) => ({
      background: bg ?? "#24292e",
      color: textColor ?? "#fff",
      border: textColor ? "1px solid #dadce0" : "none",
      borderRadius: 6,
      padding: "9px 18px",
      fontSize: 13,
      fontWeight: 600,
      cursor: loading ? "not-allowed" : "pointer",
      opacity: loading ? 0.6 : 1,
      width: "100%",
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      gap: 8,
      fontFamily: "inherit",
    }),
    radioRow: {
      display: "flex",
      gap: 20,
      marginBottom: 20,
    },
    radioLabel: (active: boolean) => ({
      fontSize: 13,
      fontWeight: 500,
      color: active ? C.t1 : C.t3,
      cursor: "pointer",
      display: "flex",
      alignItems: "center",
      gap: 6,
    }),
    errorText: {
      fontSize: 12,
      color: C.error,
      fontWeight: 500,
      marginTop: -6,
      marginBottom: 10,
    },
    defaultServerRow: {
      display: "flex",
      alignItems: "center",
      gap: 6,
      marginTop: -6,
      marginBottom: 12,
    },
    defaultServerLabel: {
      fontSize: 11,
      color: C.t3,
    },
    defaultServerBtn: {
      fontSize: 11,
      color: C.t1,
      textDecoration: "underline",
      background: "none",
      border: "none",
      padding: 0,
      cursor: "pointer",
      fontFamily: "inherit",
      fontWeight: 500,
    },
  };

  // Renders the sign-in action area based on fetched provider list.
  // - null (not fetched yet): show nothing
  // - [] (no OAuth / fetch failed): show a single generic "Sign in" button
  // - [github] / [google] / [github, google]: show matching provider buttons
  const renderBrowserActions = () => {
    if (providersLoading) {
      return (
        <div style={{ fontSize: 12, color: C.t3, marginTop: 8 }}>
          Checking relay…
        </div>
      );
    }
    if (providers === null) return null;

    if (providers.length === 0) {
      return (
        <button
          type="button"
          style={S.connectBtn(browserLoading)}
          onClick={() => handleBrowserConnect()}
          disabled={browserLoading}
        >
          {browserLoading ? "Opening browser…" : "Sign in"}
        </button>
      );
    }

    return (
      <div style={{ display: "flex", flexDirection: "column", gap: 8, marginTop: 4 }}>
        {providers.includes("github") && (
          <button
            type="button"
            style={S.providerBtn(browserLoading, "#24292e")}
            onClick={() => handleBrowserConnect("github")}
            disabled={browserLoading}
          >
            <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor" style={{ flexShrink: 0 }}>
              <path d="M12 0C5.37 0 0 5.37 0 12c0 5.31 3.435 9.795 8.205 11.385.6.105.825-.255.825-.57 0-.285-.015-1.23-.015-2.235-3.015.555-3.795-.735-4.035-1.41-.135-.345-.72-1.41-1.23-1.695-.42-.225-1.02-.78-.015-.795.945-.015 1.62.87 1.845 1.23 1.08 1.815 2.805 1.305 3.495.99.105-.78.42-1.305.765-1.605-2.67-.3-5.46-1.335-5.46-5.925 0-1.305.465-2.385 1.23-3.225-.12-.3-.54-1.53.12-3.18 0 0 1.005-.315 3.3 1.23.96-.27 1.98-.405 3-.405s2.04.135 3 .405c2.295-1.56 3.3-1.23 3.3-1.23.66 1.65.24 2.88.12 3.18.765.84 1.23 1.905 1.23 3.225 0 4.605-2.805 5.625-5.475 5.925.435.375.81 1.095.81 2.22 0 1.605-.015 2.895-.015 3.3 0 .315.225.69.825.57A12.02 12.02 0 0 0 24 12c0-6.63-5.37-12-12-12z" />
            </svg>
            {browserLoading ? "Opening browser…" : "Continue with GitHub"}
          </button>
        )}
        {providers.includes("google") && (
          <button
            type="button"
            style={S.providerBtn(browserLoading, "#fff", "#3c4043")}
            onClick={() => handleBrowserConnect("google")}
            disabled={browserLoading}
          >
            <svg width="18" height="18" viewBox="0 0 24 24" style={{ flexShrink: 0 }}>
              <path fill="#4285F4" d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z" />
              <path fill="#34A853" d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z" />
              <path fill="#FBBC05" d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l3.66-2.84z" />
              <path fill="#EA4335" d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z" />
            </svg>
            {browserLoading ? "Opening browser…" : "Continue with Google"}
          </button>
        )}
      </div>
    );
  };

  return (
    <div style={S.overlay} onClick={hideClose ? undefined : onClose} role="presentation">
      <div style={S.pane} onClick={(e) => e.stopPropagation()} role="dialog" aria-modal="true">
        <div style={S.titleRow}>
          <div style={{ fontSize: 16, fontWeight: 600 }}>
            {fromCli ? "Sign in to share with CLI" : "Connect to relay"}
          </div>
          {!hideClose && (
            <button
              type="button"
              onClick={onClose}
              aria-label="Close"
              style={{ background: "none", border: "none", cursor: "pointer", color: C.t2, padding: 4 }}
            >
              <IconX size={14} />
            </button>
          )}
        </div>

        {!fromCli && (
          <div style={S.radioRow}>
            <label style={S.radioLabel(method === "browser")}>
              <input type="radio" checked={method === "browser"} onChange={() => setMethod("browser")} style={{ accentColor: C.accent }} />
              Sign in with browser
            </label>
            <label style={S.radioLabel(method === "token")}>
              <input type="radio" checked={method === "token"} onChange={() => setMethod("token")} style={{ accentColor: C.accent }} />
              Paste pairing token
            </label>
          </div>
        )}

        {method === "browser" && (
          <>
            <div style={S.fieldLabel}>Relay URL</div>
            <input
              style={S.input(!!browserError && !browserUrl.trim())}
              placeholder="https://api.example.com"
              value={browserUrl}
              onChange={(e) => { setBrowserUrl(e.target.value); setBrowserError(null); }}
              disabled={browserLoading}
            />
            {!browserUrl && (
              <div style={S.defaultServerRow}>
                <span style={S.defaultServerLabel}>Using cinchcli.com?</span>
                <button
                  type="button"
                  style={S.defaultServerBtn}
                  onClick={() => { setBrowserUrl("https://api.cinchcli.com"); setBrowserError(null); }}
                >
                  Use api.cinchcli.com →
                </button>
              </div>
            )}
            {browserError && <div style={S.errorText}>{browserError}</div>}
            {renderBrowserActions()}
          </>
        )}

        {method === "token" && (
          <>
            <div style={S.fieldLabel}>Relay URL</div>
            <input
              style={S.input(!!tokenError && !tokenUrl.trim())}
              placeholder="https://api.example.com"
              value={tokenUrl}
              onChange={(e) => setTokenUrl(e.target.value)}
              disabled={tokenLoading}
            />
            <div style={S.fieldLabel}>Pairing token</div>
            <input
              style={S.input(!!tokenError && !token.trim())}
              placeholder="Paste token from: cinch auth login"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter") handleTokenConnect(); }}
              disabled={tokenLoading}
            />
            {tokenError && <div style={S.errorText}>{tokenError}</div>}
            <button type="button" style={S.connectBtn(tokenLoading)} onClick={handleTokenConnect} disabled={tokenLoading}>
              {tokenLoading ? "Connecting…" : "Connect"}
            </button>
          </>
        )}
      </div>
    </div>
  );
}
