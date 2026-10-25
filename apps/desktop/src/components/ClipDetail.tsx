import { useEffect, useState, type ReactNode } from 'react';
import type { LocalClip } from '../bindings';
import { formatBytes } from '../design';
import { parseColorClip } from '../lib/colorClip';
import { sourceAppIconId } from '../lib/clipIcon';
import { SourceAppIcon } from './SourceAppIcon';

interface ClipDetailProps {
  clip: LocalClip | null;
  searchQuery?: string;
  sourceDisplayNames?: Record<string, string>;
}

export function ClipDetail({ clip, searchQuery = '', sourceDisplayNames = {} }: ClipDetailProps) {
  const [imgDims, setImgDims] = useState<{ w: number; h: number } | null>(null);
  useEffect(() => { setImgDims(null); }, [clip?.id]);

  if (!clip) return <div className="clip-detail history-empty">Select a clip</div>;

  const isImage = clip.content_type === 'image';
  const appIconId = sourceAppIconId(clip);
  const color = clip.content_type === 'text' ? parseColorClip(clip.content) : null;
  const trimmed = clip.content.trim();
  const isCode = clip.content_type === 'code' ||
    (trimmed.startsWith('{') && trimmed.endsWith('}')) ||
    (trimmed.startsWith('[') && trimmed.endsWith(']'));
  const source = sourceDisplayNames[clip.source] ??
    (clip.source === 'local' ? 'This device' : clip.source.replace(/^remote:/, ''));
  const stamp = new Date(clip.created_at * 1000).toLocaleString(undefined, {
    month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
  });

  return (
    <div className="clip-detail">
      <div className="clip-content">
        {isImage ? (
          <img
            className="clip-image"
            src={`cinch://media/${clip.id}`}
            alt={`Clip from ${source}`}
            onLoad={(e) => {
              const img = e.currentTarget;
              if (img.naturalWidth) setImgDims({ w: img.naturalWidth, h: img.naturalHeight });
            }}
          />
        ) : color ? (
          <div className="clip-color">
            <div role="img" aria-label={`Color preview for ${color.value}`} style={{ backgroundColor: color.cssColor }} />
            <span>{color.value}</span>
          </div>
        ) : isCode ? (
          <pre>{highlightText(clip.content, searchQuery)}</pre>
        ) : (
          <p>{highlightText(clip.content, searchQuery)}</p>
        )}
      </div>
      <dl className="clip-metadata">
        <MetaRow label="Source" value={source} />
        {clip.source_app && (
          <MetaRow label="App" value={
            <span className="clip-app">
              {appIconId && <SourceAppIcon appId={appIconId} size={14} testId="source-app-icon" />}
              {clip.source_app}
            </span>
          } />
        )}
        {clip.source_url && <MetaRow label="URL" value={clip.source_url} />}
        <MetaRow label="Type" value={color ? 'color' : clip.content_type} />
        <MetaRow label="Captured" value={stamp} />
        <MetaRow label="Sync" value={clip.sync_state === 'pending' ? 'Sending…' : clip.synced ? 'Synced' : 'Local only'} />
        {isImage && <MetaRow label="Size" value={formatBytes(clip.byte_size)} />}
        {isImage && imgDims && <MetaRow label="Dimensions" value={`${imgDims.w} × ${imgDims.h}`} />}
        {clip.label && <MetaRow label="Label" value={clip.label} />}
        {clip.is_pinned && clip.pin_note && <MetaRow label="Note" value={clip.pin_note} />}
      </dl>
    </div>
  );
}

function highlightText(text: string, query: string): ReactNode {
  if (!query.trim()) return text;
  const escaped = query.trim().replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return text.split(new RegExp(`(${escaped})`, 'gi')).map((part, index) =>
    index % 2 === 1 ? <mark key={index}>{part}</mark> : part,
  );
}

function MetaRow({ label, value }: { label: string; value: ReactNode }) {
  return <><dt>{label}</dt><dd>{value}</dd></>;
}
