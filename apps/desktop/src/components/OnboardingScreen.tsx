import { useState, type CSSProperties } from 'react';
import { C } from '../design';
import { IconCinch } from '../icons';
import { AddRelayDialog } from './AddRelayDialog';

const DEFAULT_RELAY_HOST = 'api.cinchcli.com';
const DEFAULT_RELAY_URL = 'https://api.cinchcli.com';

interface OnboardingScreenProps {
  /// Host shown on the "Hosted on …" line. Defaults to the hosted relay.
  relayHost?: string;
  /// Opens Settings → "What the relay can see" (the honest trust disclosure).
  onShowSettings: () => void;
  /// Window-drag passthrough so the frameless window can be moved by its chrome.
  onMouseDown?: (e: React.MouseEvent) => void;
}

// First-run / signed-out surface (Surface 3, "Signal Path" direction). The
// brand slogan is the hero; a restrained hairline motif makes the AI-era
// positioning (copy → encrypted relay → agent/MCP) visible. The primary CTA
// opens the real sign-in dialog. Ports the verified mockup at
// apps/desktop/preview/redesign-mockups/onboarding-signal-path.html.
export function OnboardingScreen({
  relayHost = DEFAULT_RELAY_HOST,
  onShowSettings,
  onMouseDown,
}: OnboardingScreenProps) {
  // null = closed; 'default' pre-fills the hosted relay so OAuth buttons appear
  // immediately; 'custom' opens blank so the user can point at a self-host.
  const [signIn, setSignIn] = useState<null | 'default' | 'custom'>(null);

  return (
    <main data-testid="onboarding-root" style={S.root} onMouseDown={onMouseDown}>
      <div style={S.top}>
        <span style={S.brand}>
          <span style={S.mark}><IconCinch size={20} /></span>
          <span style={S.word}>cinch</span>
        </span>
      </div>

      <div style={S.body}>
        <div style={S.lead}>
          <div style={S.kicker}>Welcome</div>
          <h1 style={S.h1}>
            The clipboard
            <br />
            for the AI-era.
          </h1>
          <p style={S.sub}>
            Copy on one machine, paste on the next — and let your agents reach
            the same clips over MCP. Every clip is end-to-end encrypted on this
            Mac before it leaves; the relay only ever stores ciphertext.
          </p>

          <div style={S.actions}>
            <button type="button" className="onb-signin" style={S.signin} onClick={() => setSignIn('default')}>
              Sign in <span style={S.signinArrow}>→</span>
            </button>
          </div>

          <div style={S.server}>
            Hosted on {relayHost} ·
            <button type="button" className="onb-link" style={S.serverLink} onClick={() => setSignIn('custom')}>
              Use your own server
            </button>
          </div>
        </div>

        <div style={S.art} aria-hidden="true">
          <svg viewBox="0 0 300 260" fill="none" style={S.artSvg}>
            {/* flow paths (copy → relay → agent) */}
            <path d="M120 64 H180" stroke="currentColor" strokeWidth="1.1" strokeDasharray="3 4" />
            <path d="M240 96 C 240 150, 170 150, 170 182" stroke="currentColor" strokeWidth="1.1" strokeDasharray="3 4" />
            {/* travelling clip on the first leg */}
            <rect x="144" y="58" width="12" height="12" rx="2" fill="currentColor" />

            {/* node A — your mac */}
            <rect x="14" y="34" width="106" height="60" rx="6" stroke="currentColor" strokeWidth="1.25" />
            <rect x="28" y="50" width="13" height="13" rx="2" fill="currentColor" opacity="0.9" />
            <line x1="50" y1="56" x2="96" y2="56" stroke="currentColor" strokeWidth="1" opacity="0.5" />
            <line x1="50" y1="64" x2="80" y2="64" stroke="currentColor" strokeWidth="1" opacity="0.5" />
            <text x="14" y="110" style={S.nlabel}>your mac</text>

            {/* node B — relay */}
            <rect x="180" y="36" width="106" height="56" rx="6" stroke="currentColor" strokeWidth="1.25" />
            <text x="196" y="62" style={S.nlabel}>relay</text>
            <text x="196" y="76" style={S.nsub}>ciphertext only</text>

            {/* node C — agent / mcp */}
            <rect x="108" y="182" width="124" height="56" rx="6" stroke="currentColor" strokeWidth="1.25" />
            <text x="124" y="208" style={S.nlabel}>agent · mcp</text>
            <text x="124" y="222" style={S.nsub}>ssh-devbox</text>
          </svg>
        </div>
      </div>

      <div style={S.facts}>
        <span style={S.trust}>End-to-end encrypted · open-source · self-hostable.</span>
        <button type="button" className="onb-more" style={S.more} onClick={onShowSettings}>
          What can the server see? →
        </button>
      </div>

      {signIn !== null && (
        <AddRelayDialog
          onClose={() => setSignIn(null)}
          initialRelayUrl={signIn === 'default' ? DEFAULT_RELAY_URL : ''}
        />
      )}
    </main>
  );
}

const S: Record<string, CSSProperties> = {
  root: {
    background: C.bg,
    color: C.t1,
    height: '100vh',
    display: 'flex',
    flexDirection: 'column',
    position: 'relative',
    borderRadius: 'var(--radius-xl)',
    overflow: 'hidden',
    border: `1px solid ${C.border}`,
    fontFamily: 'var(--font-body)',
  },
  top: {
    display: 'flex',
    alignItems: 'center',
    padding: '26px 36px',
    flexShrink: 0,
  },
  brand: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 8,
  },
  mark: {
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    color: C.t1,
  },
  word: {
    fontFamily: 'var(--font-body)',
    fontSize: 15,
    fontWeight: 500,
    letterSpacing: '-0.01em',
    color: C.t1,
    textTransform: 'lowercase',
  },
  body: {
    flex: 1,
    minHeight: 0,
    display: 'flex',
    alignItems: 'center',
    gap: 48,
    padding: '0 64px',
  },
  lead: {
    flex: 1.3,
    minWidth: 0,
  },
  kicker: {
    fontFamily: 'var(--font-mono)',
    fontSize: 10,
    fontWeight: 400,
    letterSpacing: '0.22em',
    textTransform: 'uppercase',
    color: C.t3,
    marginBottom: 20,
  },
  h1: {
    fontFamily: 'var(--font-body)',
    fontWeight: 300,
    fontSize: 40,
    lineHeight: 1.12,
    letterSpacing: '-0.02em',
    color: C.t1,
    margin: 0,
  },
  sub: {
    marginTop: 18,
    maxWidth: '44ch',
    fontSize: 14.5,
    fontWeight: 300,
    lineHeight: 1.65,
    color: C.t2,
  },
  actions: {
    marginTop: 32,
    display: 'flex',
    alignItems: 'center',
    gap: 12,
  },
  signin: {
    fontFamily: 'var(--font-body)',
    fontSize: 13.5,
    fontWeight: 500,
    color: C.accentOn,
    background: C.accent,
    border: 'none',
    borderRadius: 7,
    padding: '11px 20px',
    cursor: 'pointer',
    display: 'inline-flex',
    alignItems: 'center',
    gap: 9,
  },
  signinArrow: {
    opacity: 0.7,
  },
  server: {
    marginTop: 24,
    fontFamily: 'var(--font-body)',
    fontSize: 12.5,
    color: C.t3,
    display: 'flex',
    alignItems: 'center',
    gap: 6,
  },
  serverLink: {
    fontFamily: 'var(--font-body)',
    fontSize: 12.5,
    color: C.t2,
    background: 'none',
    border: 'none',
    padding: '0 0 1px',
    cursor: 'pointer',
    borderBottom: `1px solid ${C.borderHover}`,
  },
  art: {
    flex: 1,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    color: C.t3,
  },
  artSvg: {
    width: 300,
    height: 'auto',
  },
  nlabel: {
    fontFamily: 'var(--font-mono)',
    fontSize: 9,
    letterSpacing: '0.14em',
    textTransform: 'uppercase',
    fill: C.t2,
  },
  nsub: {
    fontFamily: 'var(--font-mono)',
    fontSize: 8,
    letterSpacing: '0.1em',
    textTransform: 'uppercase',
    fill: C.t4,
  },
  facts: {
    borderTop: `1px solid ${C.border}`,
    padding: '16px 36px',
    flexShrink: 0,
    display: 'flex',
    alignItems: 'center',
    gap: 8,
  },
  trust: {
    fontFamily: 'var(--font-body)',
    fontSize: 12,
    fontWeight: 300,
    lineHeight: 1.6,
    color: C.t2,
  },
  more: {
    marginLeft: 'auto',
    fontFamily: 'var(--font-body)',
    fontSize: 12,
    fontWeight: 400,
    color: C.t1,
    background: 'none',
    border: 'none',
    padding: '0 0 1px',
    cursor: 'pointer',
    borderBottom: `1px solid ${C.borderHover}`,
  },
};
