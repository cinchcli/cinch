import { useEffect, useState, type CSSProperties } from 'react';
import { commands } from '../bindings';
import { C } from '../design';

/**
 * Read-only identity rows for the signed-in account. The Name is the display
 * name fetched from the OAuth provider (GitHub/Google) at login, surfaced via
 * `get_user_profile`; it is not user-editable here.
 */
export function AccountIdentity() {
  const [displayName, setDisplayName] = useState('');
  const [email, setEmail] = useState('');
  const [provider, setProvider] = useState('');
  const [userId, setUserId] = useState('');

  useEffect(() => {
    let mounted = true;
    (async () => {
      const p = await commands.getUserProfile();
      if (!mounted) return;
      setDisplayName(p.display_name);
      setEmail(p.email);
      setProvider(p.identity_provider);
      setUserId(p.user_id);
    })();
    return () => {
      mounted = false;
    };
  }, []);

  return (
    <dl style={S.dl}>
      <div style={S.dlRow}>
        <dt style={S.dt}>Name</dt>
        <dd style={S.dd}>{displayName || '—'}</dd>
      </div>
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
  );
}

const S: Record<string, CSSProperties> = {
  dl: { display: 'flex', flexDirection: 'column', gap: 0, margin: 0, padding: 0 },
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
    letterSpacing: '0.01em',
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
