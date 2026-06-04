import { useState, useEffect, useRef } from 'react';
import { commands, events } from '../bindings';
import { unwrap } from '../lib/tauri';
import { C } from '../design';
import { IconX } from '../icons';

interface AddSshMachineDialogProps {
  onClose: () => void;
  onShowToast: (message: string) => void;
  /** Pre-fill the relay URL (defaults to current active relay). */
  defaultRelayUrl?: string;
}

type Step = 'form' | 'connecting' | 'browser' | 'done' | 'error';

export function AddSshMachineDialog({
  onClose,
  onShowToast,
  defaultRelayUrl = '',
}: AddSshMachineDialogProps) {
  const [step, setStep] = useState<Step>('form');
  const [target, setTarget] = useState('');
  const [relayUrl, setRelayUrl] = useState(defaultRelayUrl);
  const [skipInstall, setSkipInstall] = useState(false);
  const [browserUrl, setBrowserUrl] = useState('');
  const [errorMsg, setErrorMsg] = useState('');
  const [sshHosts, setSshHosts] = useState<string[]>([]);
  const targetRef = useRef<HTMLInputElement>(null);
  const unsubRef = useRef<(() => void) | null>(null);

  // Focus target input on mount
  useEffect(() => {
    targetRef.current?.focus();
  }, []);

  useEffect(() => {
    let cancelled = false;
    unwrap(commands.listSshHosts())
      .then((hosts) => {
        if (!cancelled) setSshHosts(hosts);
      })
      .catch(() => {
        if (!cancelled) setSshHosts([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Cleanup event listener on unmount
  useEffect(() => {
    return () => {
      unsubRef.current?.();
    };
  }, []);

  const handleSubmit = async () => {
    const trimmedTarget = target.trim();
    if (!trimmedTarget) return;

    setStep('connecting');

    // Subscribe to SshPairMarkerFound before starting the command
    const unsub = await events.sshPairMarkerFound.listen((e) => {
      setBrowserUrl(e.payload.url);
      setStep('browser');
    });
    unsubRef.current = unsub;

    try {
      await unwrap(
        commands.pairViaSsh(
          trimmedTarget,
          relayUrl.trim() || null,
          skipInstall,
        ),
      );
      unsub();
      unsubRef.current = null;
      setStep('done');
      onShowToast(`${trimmedTarget} paired successfully`);
    } catch (e) {
      unsub();
      unsubRef.current = null;
      setErrorMsg(String(e));
      setStep('error');
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && step === 'form') handleSubmit();
    if (e.key === 'Escape') onClose();
  };

  return (
    <div style={S.overlay} onKeyDown={handleKeyDown}>
      <div style={S.dialog} role="dialog" aria-modal="true">
        <div style={S.header}>
          <span style={S.title}>Add machine via SSH</span>
          <button style={S.closeBtn} onClick={onClose} aria-label="Close">
            <IconX size={16} />
          </button>
        </div>

        {step === 'form' && (
          <div style={S.body}>
            <div style={S.field}>
              <label style={S.label}>SSH target</label>
              <input
                ref={targetRef}
                style={S.input}
                list="ssh-host-suggestions"
                placeholder="user@hostname or SSH alias"
                value={target}
                onChange={(e) => setTarget(e.target.value)}
              />
              <datalist id="ssh-host-suggestions">
                {sshHosts.map((host) => (
                  <option key={host} value={host} />
                ))}
              </datalist>
              <span style={S.hint}>
                Anything <code style={S.code}>ssh &lt;target&gt;</code> accepts
              </span>
              {sshHosts.length > 0 && (
                <div style={S.hostList} aria-label="SSH aliases">
                  {sshHosts.map((host) => (
                    <button
                      key={host}
                      type="button"
                      style={S.hostButton}
                      onClick={() => setTarget(host)}
                    >
                      {host}
                    </button>
                  ))}
                </div>
              )}
            </div>

            <div style={S.field}>
              <label style={S.label}>Relay URL (optional)</label>
              <input
                style={S.input}
                placeholder={defaultRelayUrl || 'https://api.cinchcli.com'}
                value={relayUrl}
                onChange={(e) => setRelayUrl(e.target.value)}
              />
              <span style={S.hint}>Leave blank to use the current relay</span>
            </div>

            <label style={S.checkRow}>
              <input
                type="checkbox"
                checked={skipInstall}
                onChange={(e) => setSkipInstall(e.target.checked)}
                style={{ accentColor: C.accent }}
              />
              <span style={S.checkLabel}>
                Skip cinch installation (already installed on remote)
              </span>
            </label>

            <div style={S.actions}>
              <button style={S.cancelBtn} onClick={onClose}>
                Cancel
              </button>
              <button
                style={{
                  ...S.primaryBtn,
                  opacity: target.trim() ? 1 : 0.5,
                  pointerEvents: target.trim() ? 'auto' : 'none',
                }}
                onClick={handleSubmit}
              >
                Connect
              </button>
            </div>
          </div>
        )}

        {step === 'connecting' && (
          <div style={S.body}>
            <div style={S.statusArea}>
              <div style={S.spinner} />
              <div style={S.statusText}>Connecting to {target}…</div>
              <div style={S.statusSubtext}>
                Installing cinch and starting authentication
              </div>
            </div>
          </div>
        )}

        {step === 'browser' && (
          <div style={S.body}>
            <div style={S.statusArea}>
              <div style={S.spinner} />
              <div style={S.statusText}>Browser opened</div>
              <div style={S.statusSubtext}>
                Complete sign-in in your browser, then return here.
              </div>
              {browserUrl && (
                <a
                  href={browserUrl}
                  style={S.urlLink}
                  onClick={(e) => {
                    e.preventDefault();
                    window.open(browserUrl, '_blank');
                  }}
                >
                  Open browser manually
                </a>
              )}
              <div style={{ ...S.statusSubtext, marginTop: 8 }}>
                Waiting for {target} to finish pairing…
              </div>
            </div>
          </div>
        )}

        {step === 'done' && (
          <div style={S.body}>
            <div style={S.statusArea}>
              <div style={S.successIcon}>✓</div>
              <div style={S.statusText}>{target} is ready</div>
              <div style={S.statusSubtext}>
                Run{' '}
                <code style={S.code}>
                  ssh {target} {'\''}echo hello | cinch push{'\''}
                </code>{' '}
                to test it.
              </div>
            </div>
            <div style={S.actions}>
              <button style={S.primaryBtn} onClick={onClose}>
                Done
              </button>
            </div>
          </div>
        )}

        {step === 'error' && (
          <div style={S.body}>
            <div style={S.statusArea}>
              <div style={S.errorIcon}>✕</div>
              <div style={S.statusText}>Setup failed</div>
              <div style={S.errorText}>{errorMsg}</div>
              <div style={S.statusSubtext}>
                Connect manually to debug:{' '}
                <code style={S.code}>ssh {target}</code>
              </div>
            </div>
            <div style={S.actions}>
              <button style={S.cancelBtn} onClick={onClose}>
                Close
              </button>
              <button
                style={S.primaryBtn}
                onClick={() => {
                  setStep('form');
                  setErrorMsg('');
                }}
              >
                Try again
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ─── Styles ────────────────────────────────────────────────

const S: Record<string, React.CSSProperties> = {
  overlay: {
    position: 'fixed',
    inset: 0,
    background: 'rgba(0,0,0,0.55)',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    zIndex: 1000,
  },

  dialog: {
    background: C.card,
    border: `1px solid ${C.border}`,
    borderRadius: 12,
    width: 420,
    maxWidth: '90vw',
    boxShadow: '0 24px 64px rgba(0,0,0,0.4)',
    display: 'flex',
    flexDirection: 'column',
  },

  header: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    padding: '16px 20px',
    borderBottom: `1px solid ${C.border}`,
  },

  title: {
    fontSize: 14,
    fontWeight: 600,
    color: C.t1,
    fontFamily: 'var(--font-body)',
  },

  closeBtn: {
    background: 'transparent',
    border: 'none',
    cursor: 'pointer',
    color: C.t3,
    padding: 4,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    borderRadius: 4,
  },

  body: {
    padding: '20px',
    display: 'flex',
    flexDirection: 'column',
    gap: 16,
  },

  field: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },

  label: {
    fontSize: 12,
    fontWeight: 600,
    color: C.t2,
    fontFamily: 'var(--font-body)',
    letterSpacing: '0.01em',
  },

  input: {
    fontSize: 13,
    fontFamily: 'var(--font-mono)',
    color: C.t1,
    background: C.bg,
    border: `1px solid ${C.border}`,
    borderRadius: 6,
    padding: '8px 12px',
    outline: 'none',
    width: '100%',
    boxSizing: 'border-box',
  },

  hint: {
    fontSize: 11,
    color: C.t4,
    fontFamily: 'var(--font-body)',
  },

  code: {
    fontFamily: 'var(--font-mono)',
    fontSize: 11,
    color: C.t3,
  },

  checkRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    cursor: 'pointer',
  },

  checkLabel: {
    fontSize: 12,
    color: C.t2,
    fontFamily: 'var(--font-body)',
  },

  hostList: {
    display: 'flex',
    flexWrap: 'wrap',
    gap: 6,
    marginTop: 4,
  },

  hostButton: {
    background: C.bg,
    color: C.t2,
    border: `1px solid ${C.border}`,
    borderRadius: 6,
    padding: '4px 8px',
    fontSize: 11,
    fontFamily: 'var(--font-mono)',
    cursor: 'pointer',
  },

  actions: {
    display: 'flex',
    gap: 8,
    justifyContent: 'flex-end',
    marginTop: 4,
  },

  cancelBtn: {
    background: 'transparent',
    color: C.t2,
    border: `1px solid ${C.border}`,
    borderRadius: 6,
    padding: '8px 16px',
    fontSize: 13,
    fontWeight: 500,
    cursor: 'pointer',
    fontFamily: 'var(--font-body)',
  },

  primaryBtn: {
    background: C.accent,
    color: C.accentOn,
    border: 'none',
    borderRadius: 6,
    padding: '8px 16px',
    fontSize: 13,
    fontWeight: 600,
    cursor: 'pointer',
    fontFamily: 'var(--font-body)',
  },

  statusArea: {
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    gap: 10,
    padding: '20px 0',
    textAlign: 'center',
  },

  spinner: {
    width: 28,
    height: 28,
    border: `3px solid ${C.border}`,
    borderTop: `3px solid ${C.t1}`,
    borderRadius: '50%',
    animation: 'spin 0.8s linear infinite',
  },

  statusText: {
    fontSize: 14,
    fontWeight: 600,
    color: C.t1,
    fontFamily: 'var(--font-body)',
  },

  statusSubtext: {
    fontSize: 12,
    color: C.t3,
    fontFamily: 'var(--font-body)',
    lineHeight: 1.5,
  },

  urlLink: {
    fontSize: 12,
    color: C.t1,
    fontFamily: 'var(--font-body)',
    cursor: 'pointer',
    textDecoration: 'underline',
  },

  successIcon: {
    fontSize: 28,
    color: C.t1,
    fontWeight: 700,
  },

  errorIcon: {
    fontSize: 28,
    color: C.error,
    fontWeight: 700,
  },

  errorText: {
    fontSize: 12,
    color: C.error,
    fontFamily: 'var(--font-mono)',
    wordBreak: 'break-all',
    maxWidth: 320,
    textAlign: 'center',
  },
};

// Global type augmentation for Tauri's shell plugin (used to open URLs)
declare global {
  interface Window {
    __TAURI__?: {
      shell?: {
        open: (url: string) => Promise<void>;
      };
    };
  }
}
