import { forwardRef, useState, type CSSProperties } from 'react';
import type { Device, LocalClip } from '../bindings';
import { C, formatTime, formatBytes } from '../design';
import { groupByTimeBucket } from '../lib/timeBuckets';
import type { MachineTagColorMap } from '../lib/machineTagColors';
import { SourcePill } from './SourcePill';
import { IconPin } from '../icons';

interface ClipListProps {
  clips: LocalClip[];
  selected: LocalClip | null;
  onSelect: (clip: LocalClip) => void;
  onCopy: (clip: LocalClip) => void;
  onSend: (clip: LocalClip, targetDeviceId: string | null) => void;
  devices: Device[];
  query: string;
  deviceNicknames: Record<string, string>;
  tagColors?: MachineTagColorMap;
  now?: number;
}

export const ClipList = forwardRef<HTMLDivElement, ClipListProps>(
  ({ clips, selected, onSelect, onCopy, onSend, devices, query, deviceNicknames, tagColors = {}, now }, ref) => {
    if (clips.length === 0) {
      return (
        <div style={S.col}>
          <div style={S.empty}>
            <div style={S.emptyTitle}>
              {query ? `No results for "${query}"` : 'No clips yet'}
            </div>
            {!query && (
              <code style={S.emptyHint}>echo "hello" | cinch push</code>
            )}
          </div>
        </div>
      );
    }

    const groups = groupByTimeBucket(clips, now);

    return (
      <div ref={ref} style={S.col} role="list">
        {groups.map(({ bucket, items }) => (
          <section key={bucket}>
            <div style={S.sectionLabel}>{bucket}</div>
            {items.map((clip) => (
              <ClipRow
                key={clip.id}
                clip={clip}
                selected={selected?.id === clip.id}
                onClick={() => onSelect(clip)}
                onDoubleClick={() => onCopy(clip)}
                onSend={(targetDeviceId) => onSend(clip, targetDeviceId)}
                devices={devices}
                nickname={deviceNicknames[clip.source]}
                colorSlot={tagColors[clip.source]}
              />
            ))}
          </section>
        ))}
      </div>
    );
  }
);

ClipList.displayName = 'ClipList';

interface ClipRowProps {
  clip: LocalClip;
  selected: boolean;
  onClick: () => void;
  onDoubleClick: () => void;
  onSend: (targetDeviceId: string | null) => void;
  devices: Device[];
  nickname?: string;
  colorSlot?: MachineTagColorMap[string];
}

function syncStateLabel(s: string): string {
  if (s === 'pending') return 'Sending…';
  if (s === 'synced') return 'Sent';
  return ''; // local: no badge (private to this device)
}

function deviceLabel(device: Device): string {
  return device.nickname ?? device.hostname ?? device.id ?? 'Unknown device';
}

function ClipRow({ clip, selected, onClick, onDoubleClick, onSend, devices, nickname, colorSlot }: ClipRowProps) {
  const [pickerOpen, setPickerOpen] = useState(false);
  const isImage = clip.content_type === 'image';
  const recency = clip.received_at && clip.received_at > 0 ? clip.received_at : clip.created_at;
  const preview = isImage
    ? `Image (${formatBytes(clip.byte_size)})`
    : clip.content.replace(/\s+/g, ' ').trim().substring(0, 140);

  const targetableDevices = devices.filter((d) => d.id !== undefined);

  return (
    <div
      role="button"
      data-id={clip.id}
      aria-selected={selected}
      aria-label={preview || 'empty clip'}
      tabIndex={0}
      className="clip-row"
      style={{ ...S.row, ...(selected ? S.rowActive : {}) }}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onClick();
        }
      }}
      onDoubleClick={onDoubleClick}
    >
      <span data-testid="clip-meta" style={S.meta}>
        <SourcePill source={clip.source} status={clip.source === 'local' ? 'local' : 'remote'} nickname={nickname} colorSlot={colorSlot} />
        <span style={{ color: C.t4 }}>·</span>
        <span>{formatTime(recency)}</span>
        {syncStateLabel(clip.sync_state) && (
          <span data-testid="clip-sync-state" style={S.syncState}>
            {syncStateLabel(clip.sync_state)}
          </span>
        )}
        {clip.is_pinned && (
          <span data-testid="clip-pin-indicator" style={S.pinIndicator} aria-label="Pinned">
            <IconPin size={11} />
          </span>
        )}
      </span>
      <span data-testid="clip-preview" style={S.preview}>{preview || ' '}</span>
      <span style={S.sendGroup}>
        <button
          aria-label="Send clip"
          className="clip-row-send"
          onClick={(e) => {
            e.stopPropagation();
            onSend(null);
          }}
          style={S.sendBtn}
        >
          Send
        </button>
        <button
          aria-label="Send to a specific device"
          className="clip-row-send-to"
          onClick={(e) => {
            e.stopPropagation();
            setPickerOpen((open) => !open);
          }}
          style={S.sendToBtn}
        >
          Send to…
        </button>
        {pickerOpen && (
          <div role="menu" style={S.picker}>
            {targetableDevices.length === 0 ? (
              <div style={S.pickerEmpty}>No devices</div>
            ) : (
              targetableDevices.map((device) => {
                const label = deviceLabel(device);
                const isOnline = device.online === true;
                return (
                  <button
                    key={device.id}
                    role="menuitem"
                    aria-label={label}
                    onClick={(e) => {
                      e.stopPropagation();
                      setPickerOpen(false);
                      onSend(device.id!);
                    }}
                    style={S.pickerItem}
                  >
                    <span
                      style={{
                        ...S.onlineDot,
                        background: isOnline ? '#34c759' : '#8e8e93',
                      }}
                      aria-hidden="true"
                    />
                    <span>{label}</span>
                    {!isOnline && (
                      <span style={S.offlineLabel}>(offline)</span>
                    )}
                  </button>
                );
              })
            )}
          </div>
        )}
      </span>
    </div>
  );
}

const S: Record<string, CSSProperties> = {
  col: {
    width: 'var(--list-width, 320px)',
    flexShrink: 0,
    background: C.card,
    borderRight: `1px solid ${C.border}`,
    overflowY: 'auto',
  },
  sectionLabel: {
    padding: 'var(--sp-md) var(--sp-lg) var(--sp-sm)',
    fontFamily: 'var(--font-body)',
    fontSize: 11,
    fontWeight: 500,
    letterSpacing: '0.01em',
    color: C.t3,
  },
  row: {
    position: 'relative',
    padding: 'var(--sp-md) var(--sp-lg)',
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
    cursor: 'pointer',
    borderBottom: `1px solid ${C.border}`,
    outline: 'none',
  },
  rowActive: {
    background: C.selected,
  },
  preview: {
    fontSize: 13.5,
    fontFamily: 'var(--font-body)',
    color: C.t1,
    display: '-webkit-box',
    WebkitBoxOrient: 'vertical',
    WebkitLineClamp: 2,
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    letterSpacing: '-0.005em',
    lineHeight: 1.45,
    wordBreak: 'break-word',
  },
  meta: {
    display: 'flex',
    alignItems: 'center',
    gap: 6,
    fontSize: 10.5,
    fontFamily: 'var(--font-mono)',
    letterSpacing: '0.04em',
    color: C.t3,
  },
  syncState: {
    color: C.t3,
    marginLeft: 6,
  },
  pinIndicator: {
    marginLeft: 'auto',
    display: 'inline-flex',
    alignItems: 'center',
    color: 'var(--accent)',
  },
  sendGroup: {
    position: 'absolute',
    right: 'var(--sp-lg)',
    top: 'var(--sp-md)',
    display: 'flex',
    gap: 4,
    alignItems: 'center',
  },
  sendBtn: {
    background: 'none',
    border: `1px solid ${C.border}`,
    borderRadius: 4,
    color: C.t2,
    fontSize: 11,
    padding: '2px 8px',
    cursor: 'pointer',
  },
  sendToBtn: {
    background: 'none',
    border: `1px solid ${C.border}`,
    borderRadius: 4,
    color: C.t3,
    fontSize: 11,
    padding: '2px 8px',
    cursor: 'pointer',
  },
  picker: {
    position: 'absolute',
    top: '100%',
    right: 0,
    marginTop: 4,
    background: C.card,
    border: `1px solid ${C.border}`,
    borderRadius: 6,
    boxShadow: '0 4px 16px rgba(0,0,0,0.12)',
    minWidth: 160,
    zIndex: 100,
    overflow: 'hidden',
  },
  pickerItem: {
    display: 'flex',
    alignItems: 'center',
    gap: 6,
    width: '100%',
    background: 'none',
    border: 'none',
    padding: '6px 10px',
    fontSize: 12,
    color: C.t1,
    cursor: 'pointer',
    textAlign: 'left',
  },
  pickerEmpty: {
    padding: '6px 10px',
    fontSize: 12,
    color: C.t3,
  },
  onlineDot: {
    width: 7,
    height: 7,
    borderRadius: '50%',
    flexShrink: 0,
  },
  offlineLabel: {
    color: C.t3,
    marginLeft: 2,
    fontSize: 11,
  },
  empty: {
    padding: '40px var(--sp-xl)',
    textAlign: 'center',
  },
  emptyTitle: { color: C.t2, fontSize: 13, marginBottom: 6 },
  emptyHint: { fontSize: 11, color: C.t3, fontFamily: 'var(--font-mono)' },
};
