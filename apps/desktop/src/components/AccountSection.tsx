import { useEffect, useState } from 'react';
import { commands } from '../bindings';
import { C } from '../design';

export function AccountSection() {
  const [displayName, setDisplayName] = useState('');
  const [email, setEmail] = useState('');
  const [provider, setProvider] = useState('');
  const [userId, setUserId] = useState('');
  const [draft, setDraft] = useState('');
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [savedFlash, setSavedFlash] = useState(false);

  useEffect(() => {
    let mounted = true;
    (async () => {
      const p = await commands.getUserProfile();
      if (!mounted) return;
      setDisplayName(p.display_name);
      setDraft(p.display_name);
      setEmail(p.email);
      setProvider(p.identity_provider);
      setUserId(p.user_id);
    })();
    return () => {
      mounted = false;
    };
  }, []);

  const onSave = async () => {
    const trimmed = draft.trim();
    if (trimmed.length === 0) {
      setError('Display name must not be empty.');
      return;
    }
    if (trimmed.length > 64) {
      setError('Display name must be 64 characters or fewer.');
      return;
    }
    setError(null);
    setSaving(true);
    try {
      // setDisplayName returns typedError<string, string>:
      // { status: "ok"; data: string } | { status: "error"; error: string }
      const result = await commands.setDisplayName(trimmed);
      if (result.status === 'error') {
        setError(result.error);
        return;
      }
      const stored = result.data;
      setDisplayName(stored);
      setDraft(stored);
      setSavedFlash(true);
      setTimeout(() => setSavedFlash(false), 1500);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const dirty = draft.trim() !== displayName.trim();

  return (
    <section aria-label="Account settings">
      <div style={S.fieldGroup}>
        <label htmlFor="display-name-input" style={S.label}>
          Display name
        </label>
        <div style={S.inputRow}>
          <input
            id="display-name-input"
            type="text"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            maxLength={64}
            style={S.input}
            aria-describedby={error ? 'display-name-error' : undefined}
          />
          <button
            type="button"
            onClick={onSave}
            disabled={!dirty || saving}
            style={!dirty || saving ? S.primaryBtnDisabled : S.primaryBtn}
          >
            {saving ? 'Saving…' : 'Save'}
          </button>
        </div>
        {error && (
          <p id="display-name-error" role="alert" style={S.errorMsg}>
            {error}
          </p>
        )}
        {savedFlash && (
          <p role="status" style={S.savedMsg}>
            Saved.
          </p>
        )}
      </div>

      <dl style={S.dl}>
        <div style={S.dlRow}>
          <dt style={S.dt}>Email</dt>
          <dd style={S.dd}>{email || '—'}</dd>
        </div>
        <div style={S.dlRow}>
          <dt style={S.dt}>Provider</dt>
          <dd style={S.dd}>{provider || '—'}</dd>
        </div>
        <div style={S.dlRow}>
          <dt style={S.dt}>User ID</dt>
          <dd style={S.dd}>
            <code style={S.mono}>{userId || '—'}</code>
          </dd>
        </div>
      </dl>
    </section>
  );
}

// ─── Styles ────────────────────────────────────────────────

const S: Record<string, React.CSSProperties> = {
  fieldGroup: {
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
    marginBottom: 24,
  },

  label: {
    fontSize: 12,
    fontWeight: 600,
    color: C.t2,
    fontFamily: 'var(--font-body)',
    textTransform: 'uppercase',
    letterSpacing: '0.06em',
  },

  inputRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
  },

  input: {
    fontSize: 13,
    fontFamily: 'var(--font-body)',
    color: C.t1,
    background: C.bg,
    border: `1px solid ${C.border}`,
    borderRadius: 6,
    padding: '8px 12px',
    outline: 'none',
    flex: 1,
    boxSizing: 'border-box',
  },

  primaryBtn: {
    background: C.accent,
    color: '#fff',
    border: 'none',
    borderRadius: 6,
    padding: '8px 16px',
    fontSize: 13,
    fontWeight: 600,
    cursor: 'pointer',
    fontFamily: 'var(--font-body)',
    flexShrink: 0,
  },

  primaryBtnDisabled: {
    background: C.border,
    color: C.t3,
    border: 'none',
    borderRadius: 6,
    padding: '8px 16px',
    fontSize: 13,
    fontWeight: 600,
    cursor: 'not-allowed',
    fontFamily: 'var(--font-body)',
    flexShrink: 0,
  },

  errorMsg: {
    fontSize: 12,
    fontWeight: 500,
    color: C.error,
    margin: '4px 0 0',
  },

  savedMsg: {
    fontSize: 12,
    fontWeight: 500,
    color: C.success,
    margin: '4px 0 0',
  },

  dl: {
    display: 'flex',
    flexDirection: 'column',
    gap: 0,
    margin: 0,
    padding: 0,
  },

  dlRow: {
    display: 'flex',
    alignItems: 'baseline',
    gap: 12,
    padding: '9px 0',
    borderBottom: `1px solid ${C.border}`,
  },

  dt: {
    fontSize: 12,
    fontWeight: 600,
    color: C.t3,
    textTransform: 'uppercase',
    letterSpacing: '0.06em',
    minWidth: 72,
    flexShrink: 0,
  },

  dd: {
    fontSize: 13,
    fontWeight: 400,
    color: C.t1,
    margin: 0,
    fontFamily: 'var(--font-body)',
    wordBreak: 'break-all',
  },

  mono: {
    fontFamily: 'var(--font-mono)',
    fontSize: 12,
    color: C.t2,
    letterSpacing: '0.2px',
  },
};
