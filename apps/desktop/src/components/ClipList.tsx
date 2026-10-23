import { forwardRef } from 'react';
import type { LocalClip } from '../bindings';
import { IconPin, IconSearch, typeGlyph } from '../icons';
import { groupByTimeBucket } from '../lib/timeBuckets';
import { formatBytes } from '../design';

interface ClipListProps {
  clips: LocalClip[];
  selected: LocalClip | null;
  onSelect: (clip: LocalClip) => void;
  onCopy: (clip: LocalClip) => void;
  deviceNicknames?: Record<string, string>;
  query: string;
  now?: number;
  pinnedOnly?: boolean;
}

export const ClipList = forwardRef<HTMLDivElement, ClipListProps>(
  ({ clips, selected, onSelect, onCopy, deviceNicknames = {}, query, now, pinnedOnly = false }, ref) => {
    if (clips.length === 0) {
      return (
        <div ref={ref} className="history-list history-empty" aria-label="Clipboard history">
          <IconSearch size={20} />
          <p>{query ? `No results for "${query}"` : pinnedOnly ? 'No pinned clips' : 'No clips yet'}</p>
        </div>
      );
    }

    const groups = pinnedOnly ? [{ bucket: '', items: clips }] : groupByTimeBucket(clips, now);
    return (
      <div ref={ref} className="history-list" aria-label="Clipboard history">
        {groups.map((group) => (
          <section key={group.bucket}>
            {group.bucket && <div className="history-group">{group.bucket}</div>}
            {group.items.map((clip) => {
              const preview = clip.content_type === 'image'
                ? clip.label || `Image (${formatBytes(clip.byte_size)})`
                : clip.content.replace(/\s+/g, ' ').trim().slice(0, 140) || `Blank · ${formatBytes(clip.byte_size)}`;
              return (
                <button
                  key={clip.id}
                  type="button"
                  className="clip-row"
                  data-id={clip.id}
                  data-source={clip.source}
                  aria-label={preview}
                  aria-current={selected?.id === clip.id}
                  title={clip.source_app || deviceNicknames[clip.source] || clip.source}
                  onClick={() => onSelect(clip)}
                  onDoubleClick={() => onCopy(clip)}
                >
                  <span className="clip-row-icon" aria-hidden="true">{typeGlyph(clip.content_type, 15)}</span>
                  <span className="clip-row-label">{preview}</span>
                  {clip.sync_state === 'pending' && <span className="clip-row-status">Sending…</span>}
                  {clip.is_pinned && <span className="clip-row-icon" title="Pinned"><IconPin size={12} /></span>}
                </button>
              );
            })}
          </section>
        ))}
      </div>
    );
  },
);
ClipList.displayName = 'ClipList';
