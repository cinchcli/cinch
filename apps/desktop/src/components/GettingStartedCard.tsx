import { C } from '../design';
import { dialogStyles } from './dialogPrimitives';

interface GettingStartedCardProps {
  onConnectRelay: () => void;
}

export function GettingStartedCard({ onConnectRelay }: GettingStartedCardProps) {
  return (
    <section
      data-testid="getting-started-card"
      aria-label="Getting started"
      style={{
        flex: 1,
        overflowY: 'auto',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        padding: 32,
      }}
    >
      <div style={{ maxWidth: 420 }}>
        <h2 style={{ fontSize: 22, fontWeight: 500, color: C.t1, margin: '0 0 16px' }}>
          Your clipboard, on this device.
        </h2>
        <p style={{ fontSize: 14, color: C.t2, lineHeight: 1.7 }}>
          Copy text or an image in another app. It appears here so you can search,
          copy it again, or pin it for later.
        </p>
        <p style={{ fontSize: 12, color: C.t3, lineHeight: 1.7, marginBottom: 24 }}>
          No account, relay, or internet connection is required for local history.
          Clips are sent to a relay only when you choose Send.
        </p>
        <button type="button" style={dialogStyles.btnGhost} onClick={onConnectRelay}>
          Connect a relay (optional)
        </button>
      </div>
    </section>
  );
}
