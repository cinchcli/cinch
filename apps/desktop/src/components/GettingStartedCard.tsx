import { useState } from 'react';
import { C } from '../design';

const STORAGE_KEY = 'cinchGettingStartedDismissed';
const SNIPPET = 'echo "hello cinch" | cinch push';
const SSH_SNIPPET = 'cinch pair user@host';
const CASK_INSTALL = 'brew install --cask cinchcli/tap/cinchcli';

interface GettingStartedCardProps {
  onCopySnippet: (text: string) => void;
}

export function GettingStartedCard({ onCopySnippet }: GettingStartedCardProps) {
  const [dismissed, setDismissed] = useState<boolean>(
    () => localStorage.getItem(STORAGE_KEY) === '1',
  );

  if (dismissed) return null;

  const handleDismiss = () => {
    localStorage.setItem(STORAGE_KEY, '1');
    setDismissed(true);
  };

  return (
    <div
      data-testid="getting-started-card"
      style={{
        flex: 1,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: '32px 16px',
      }}
    >
      <div
        style={{
          maxWidth: 360,
          background: C.card2,
          border: `1px solid ${C.border}`,
          borderRadius: 8,
          padding: 20,
          position: 'relative',
        }}
      >
        <button
          onClick={handleDismiss}
          aria-label="Dismiss"
          style={{
            position: 'absolute',
            top: 8,
            right: 10,
            background: 'transparent',
            border: 'none',
            color: C.t3,
            cursor: 'pointer',
            fontSize: 11,
            padding: '2px 6px',
          }}
        >
          Dismiss
        </button>

        <div
          style={{
            fontSize: 16,
            fontWeight: 600,
            color: C.t1,
            letterSpacing: '-0.012em',
            marginBottom: 12,
          }}
        >
          You're signed in. Now send your first clip.
        </div>

        <div style={{ marginBottom: 14 }}>
          <div style={{ fontSize: 11, color: C.t3, marginBottom: 6 }}>Try it now:</div>
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              background: C.bg,
              border: `1px solid ${C.border}`,
              borderRadius: 4,
              padding: '8px 10px',
              fontFamily: 'var(--font-mono)',
              fontSize: 12,
              color: C.t1,
            }}
          >
            <code style={{ flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
              {SNIPPET}
            </code>
            <button
              onClick={() => onCopySnippet(SNIPPET)}
              aria-label="Copy"
              style={{
                background: 'transparent',
                border: `1px solid ${C.border}`,
                borderRadius: 3,
                color: C.t2,
                cursor: 'pointer',
                fontSize: 11,
                padding: '2px 8px',
              }}
            >
              Copy
            </button>
          </div>
        </div>

        <div style={{ fontSize: 12, color: C.t2, lineHeight: 1.5 }}>
          <div style={{ fontSize: 11, color: C.t3, marginBottom: 6 }}>Add another device:</div>

          <div style={{ marginBottom: 10 }}>
            <div style={{ fontSize: 11, color: C.t3, marginBottom: 4 }}>Server (SSH):</div>
            <div
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 8,
                background: C.bg,
                border: `1px solid ${C.border}`,
                borderRadius: 4,
                padding: '8px 10px',
                fontFamily: 'var(--font-mono)',
                fontSize: 12,
                color: C.t1,
              }}
            >
              <code style={{ flex: 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
                {SSH_SNIPPET}
              </code>
              <button
                onClick={() => onCopySnippet(SSH_SNIPPET)}
                aria-label="Copy"
                style={{
                  background: 'transparent',
                  border: `1px solid ${C.border}`,
                  borderRadius: 3,
                  color: C.t2,
                  cursor: 'pointer',
                  fontSize: 11,
                  padding: '2px 8px',
                }}
              >
                Copy
              </button>
            </div>
          </div>

          <div>
            <div style={{ fontSize: 11, color: C.t3, marginBottom: 4 }}>Another Mac:</div>
            <code style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: C.t1 }}>
              {CASK_INSTALL}
            </code>
          </div>
        </div>
      </div>
    </div>
  );
}
