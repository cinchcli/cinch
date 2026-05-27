import type { CSSProperties, RefObject } from 'react';
import type { LocalClip } from '../bindings';
import { C, formatTime } from '../design';
import type { MachineTagColorMap } from '../lib/machineTagColors';
import { SourcePill } from './SourcePill';
import { ClipDetail } from './ClipDetail';

interface PinnedPanelProps {
  clips: LocalClip[];
  selected: LocalClip | null;
  onSelect: (clip: LocalClip) => void;
  onCopy: (clip: LocalClip) => void;
  onPin: (clip: LocalClip) => void;
  onUnpin: (clip: LocalClip) => void;
  onDelete: (clip: LocalClip) => void;
  onSaveImage: (clip: LocalClip) => void;
  query: string;
  deviceNicknames: Record<string, string>;
  tagColors?: MachineTagColorMap;
  listRef: RefObject<HTMLDivElement | null>;
}

export function PinnedPanel({
  clips, selected, onSelect, onCopy, onPin, onUnpin, onDelete, onSaveImage,
  query, deviceNicknames, tagColors = {}, listRef,
}: PinnedPanelProps) {
  const groups = groupByPinNote(clips);

  return (
    <>
      <div ref={listRef} style={S.col}>
        {clips.length === 0 ? (
          <div style={S.empty}>
            <div style={S.emptyTitle}>
              {query ? `No pinned clips matching "${query}"` : 'No pinned clips yet'}
            </div>
            {!query && (
              <div style={S.emptyHint}>
                Press <kbd style={S.kbd}>⌘P</kbd> on any clip to pin it.
              </div>
            )}
          </div>
        ) : (
          groups.map(({ note, items }) => (
            <section key={note}>
              <div style={S.sectionLabel}>{note}</div>
              {items.map((clip) => (
                <PinnedRow
                  key={clip.id}
                  clip={clip}
                  selected={selected?.id === clip.id}
                  onClick={() => onSelect(clip)}
                  onDoubleClick={() => onCopy(clip)}
                  nickname={deviceNicknames[clip.source]}
                  colorSlot={tagColors[clip.source]}
                />
              ))}
            </section>
          ))
        )}
      </div>

      <ClipDetail
        clip={selected}
        onCopy={onCopy}
        onOpenPromptPack={() => {}}
        onPin={(c) => c.is_pinned ? onUnpin(c) : onPin(c)}
        onDelete={onDelete}
        onSaveImage={onSaveImage}
        canPromptPack={false}
        tagColors={tagColors}
        sourceDisplayNames={deviceNicknames}
      />
    </>
  );
}

interface NoteGroup { note: string; items: LocalClip[]; }

function groupByPinNote(clips: LocalClip[]): NoteGroup[] {
  const map = new Map<string, LocalClip[]>();
  const order: string[] = [];
  for (const c of clips) {
    const key = c.pin_note?.trim() || 'Unnamed';
    if (!map.has(key)) { map.set(key, []); order.push(key); }
    map.get(key)!.push(c);
  }
  return order.map(note => ({ note, items: map.get(note)! }));
}

function PinnedRow({
  clip, selected, onClick, onDoubleClick, nickname, colorSlot,
}: {
  clip: LocalClip;
  selected: boolean;
  onClick: () => void;
  onDoubleClick: () => void;
  nickname?: string;
  colorSlot?: MachineTagColorMap[string];
}) {
  const preview = clip.content.replace(/\s+/g, ' ').trim().substring(0, 140);
  return (
    <div
      role="button"
      data-id={clip.id}
      aria-selected={selected}
      aria-label={preview || 'pinned clip'}
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
      <span style={S.preview}>{preview || ' '}</span>
      <span style={S.meta}>
        <SourcePill source={clip.source} status={clip.source === 'local' ? 'local' : 'remote'} nickname={nickname} colorSlot={colorSlot} />
        <span style={{ color: C.t4 }}>·</span>
        <span>{formatTime(clip.created_at)}</span>
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
    padding: '16px 18px 8px',
    fontSize: 11,
    fontWeight: 600,
    letterSpacing: '-0.005em',
    color: C.t2,
  },
  row: {
    padding: '11px 18px',
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
    whiteSpace: 'nowrap',
    overflow: 'hidden',
    textOverflow: 'ellipsis',
    letterSpacing: '-0.005em',
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
  empty: { padding: '40px 20px', textAlign: 'center' },
  emptyTitle: { color: C.t2, fontSize: 13, marginBottom: 6 },
  emptyHint: { fontSize: 11, color: C.t3 },
  kbd: {
    fontFamily: 'var(--font-mono)',
    fontSize: 9.5,
    padding: '1px 5px',
    background: 'var(--kbd-bg)',
    border: '1px solid var(--kbd-border)',
    color: 'var(--kbd-color)',
    borderRadius: 3,
  },
};
