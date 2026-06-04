import type { CSSProperties } from 'react';
import { C } from '../design';
import { IconInbox, IconPin, IconMonitor, IconGear, IconCinch } from '../icons';

export type RailPanel = 'inbox' | 'pinned' | 'devices';

interface RailProps {
  active: RailPanel;
  onSelect: (panel: RailPanel) => void;
  onOpenSettings: () => void;
}

interface RailItemProps {
  label: string;
  active?: boolean;
  onClick: () => void;
  children: React.ReactNode;
}

function RailItem({ label, active, onClick, children }: RailItemProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-current={active ? 'page' : undefined}
      onClick={onClick}
      className="rail-btn"
      style={{
        ...S.ic,
        ...(active ? S.icActive : {}),
      }}
    >
      {active && <span style={S.activeBar} aria-hidden="true" />}
      {children}
    </button>
  );
}

export function Rail({ active, onSelect, onOpenSettings }: RailProps) {
  return (
    <nav aria-label="Sections" style={S.rail}>
      <span style={S.mark} aria-hidden="true">
        <IconCinch size={20} />
      </span>
      <RailItem label="Inbox" active={active === 'inbox'} onClick={() => onSelect('inbox')}>
        <IconInbox size={20} />
      </RailItem>
      <RailItem label="Pinned" active={active === 'pinned'} onClick={() => onSelect('pinned')}>
        <IconPin size={20} />
      </RailItem>
      <RailItem label="Devices" active={active === 'devices'} onClick={() => onSelect('devices')}>
        <IconMonitor size={20} />
      </RailItem>
      <span style={{ flex: 1 }} aria-hidden="true" />
      <RailItem label="Settings" onClick={onOpenSettings}>
        <IconGear size={20} />
      </RailItem>
    </nav>
  );
}

const S: Record<string, CSSProperties> = {
  rail: {
    width: 56,
    background: C.card,
    borderRight: `1px solid ${C.border}`,
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    padding: '14px 0',
    gap: 4,
    flexShrink: 0,
  },
  mark: {
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    color: C.t1,
    marginBottom: 18,
    flexShrink: 0,
  },
  ic: {
    width: 36,
    height: 36,
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
    background: 'transparent',
    border: 'none',
    color: C.t3,
    borderRadius: 6,
    cursor: 'pointer',
    transition: 'color 100ms ease, background 100ms ease',
    position: 'relative',
  },
  icActive: {
    background: C.card2,
    color: C.accent,
  },
  activeBar: {
    position: 'absolute',
    left: -10,
    top: '50%',
    transform: 'translateY(-50%)',
    width: 2,
    height: 18,
    background: 'var(--accent)',
    borderRadius: 2,
  },
};
