import type { CSSProperties, ReactNode } from 'react';
import { useEffect, useState } from 'react';
import type { LocalClip } from '../bindings';
import { C, formatBytes } from '../design';
import { parseColorClip, type ParsedColorClip } from '../lib/colorClip';
import type { MachineTagColorMap } from '../lib/machineTagColors';
import { SourcePill } from './SourcePill';

interface ClipDetailProps {
  clip: LocalClip | null;
  onCopy: (clip: LocalClip) => void;
  onPin: (clip: LocalClip) => void;
  onDelete: (clip: LocalClip) => void;
  onSaveImage: (clip: LocalClip) => void;
  searchQuery?: string;
  tagColors?: MachineTagColorMap;
  sourceDisplayNames?: Record<string, string>;
}

export function ClipDetail({
  clip,
  onCopy,
  onPin,
  onDelete,
  onSaveImage,
  searchQuery,
  tagColors = {},
  sourceDisplayNames = {},
}: ClipDetailProps) {
  const [imgDims, setImgDims] = useState<{ w: number; h: number } | null>(null);
  useEffect(() => { setImgDims(null); }, [clip?.id]);

  if (!clip) {
    return (
      <div style={S.placeholder}>
        <span style={S.placeholderTitle}>Select a clip</span>
        <span style={S.placeholderHint}>↵ copy · ⌘P pin · ⌘⌫ delete</span>
      </div>
    );
  }

  const isImage = clip.content_type === 'image';
  const colorClip = clip.content_type === 'text' ? parseColorClip(clip.content) : null;
  const trimmed = clip.content.trim();
  const isJsonish =
    (trimmed.startsWith('{') && trimmed.endsWith('}')) ||
    (trimmed.startsWith('[') && trimmed.endsWith(']'));
  // Prose for free-form text only; structured content (json-shaped or code/url)
  // stays in mono so whitespace and punctuation read correctly.
  const isProse = !isJsonish && clip.content_type === 'text';
  const body = isJsonish ? tryPrettyJson(clip.content) : clip.content;
  const highlightQuery = (searchQuery ?? '').trim();

  const stamp = new Date(clip.created_at * 1000).toLocaleString(undefined, {
    month: 'short', day: 'numeric', year: 'numeric',
    hour: '2-digit', minute: '2-digit', second: '2-digit',
  });

  return (
    <div style={S.col}>
      <div style={S.header}>
        <div style={S.stamp}>
          <SourcePill
            source={clip.source}
            status={clip.source === 'local' ? 'local' : 'remote'}
            nickname={sourceDisplayNames[clip.source]}
            colorSlot={tagColors[clip.source]}
          />
          {clip.label && clip.label.length > 0 && (
            <>
              <span style={{ color: C.t4 }}>·</span>
              <span style={{
                background: C.card2,
                color: C.t2,
                fontSize: 10,
                fontWeight: 500,
                padding: '1px 8px',
                borderRadius: 999,
                letterSpacing: '0.02em',
              }}>
                {clip.label}
              </span>
            </>
          )}
          <span style={{ color: C.t4 }}>·</span>
          <span>{stamp}</span>
        </div>
      </div>

      {isImage ? (
        <div style={S.imageStage}>
          <img
            src={`cinch://media/${clip.id}`}
            alt={`Clip from ${clip.source}`}
            style={S.imageFit}
            onLoad={(e) => {
              const img = e.currentTarget;
              if (img.naturalWidth) setImgDims({ w: img.naturalWidth, h: img.naturalHeight });
            }}
          />
        </div>
      ) : colorClip ? (
        <ColorPreview color={colorClip} />
      ) : (
        <div style={S.scrollArea}>
          {isProse ? (
            <div style={S.prose}>{highlightText(body, highlightQuery)}</div>
          ) : (
            <pre style={S.code}>{highlightText(body, highlightQuery)}</pre>
          )}
        </div>
      )}

      <div style={S.footer}>
        <div style={S.actions}>
          <button type="button" onClick={() => onCopy(clip)} className="btn-primary" style={S.btnPrimary}>
            Copy <span style={S.kbdHint}>↵</span>
          </button>
          <button type="button" onClick={() => onPin(clip)} className="btn-ghost" style={S.btnGhost}>
            {clip.is_pinned ? 'Unpin' : 'Pin'} <span style={S.kbdHint}>⌘P</span>
          </button>
          {isImage && (
            <button
              type="button"
              onClick={() => onSaveImage(clip)}
              className="btn-ghost"
              style={S.btnGhost}
            >
              Save…
            </button>
          )}
          <button
            type="button"
            onClick={() => onDelete(clip)}
            className="btn-ghost"
            style={{ ...S.btnGhost, marginLeft: 'auto' }}
          >
            Delete <span style={S.kbdHint}>⌘⌫</span>
          </button>
        </div>

        <dl style={S.metaList}>
          <MetaRow label="Source" value={clip.source.startsWith('remote:') ? clip.source.replace('remote:', '') : clip.source} />
          {clip.source_app && <MetaRow label="App" value={<AppMeta clip={clip} />} />}
          {clip.source_url && <MetaRow label="URL" value={clip.source_url} />}
          <MetaRow label="Type" value={colorClip ? 'color' : clip.content_type} />
          <MetaRow label="Size" value={formatBytes(clip.byte_size)} />
          {isImage && imgDims && <MetaRow label="Dimensions" value={`${imgDims.w} × ${imgDims.h}`} />}
          {clip.is_pinned && <MetaRow label="Note" value={clip.pin_note ?? '(no note)'} />}
        </dl>
      </div>
    </div>
  );
}

function ColorPreview({ color }: { color: ParsedColorClip }) {
  return (
    <div style={S.colorStage}>
      <div
        aria-label={`Color preview for ${color.value}`}
        role="img"
        style={{
          ...S.colorSwatchLarge,
          backgroundColor: color.cssColor,
        }}
      />
      <div style={S.colorValue}>{color.value}</div>
    </div>
  );
}

function AppMeta({ clip }: { clip: LocalClip }) {
  return (
    <span style={S.appMeta}>
      {clip.source_app_id && (
        <img
          data-testid="source-app-icon"
          src={`cinch://app-icon/${encodeURIComponent(clip.source_app_id)}`}
          alt=""
          aria-hidden="true"
          style={S.appIcon}
        />
      )}
      <span>{clip.source_app}</span>
    </span>
  );
}

function MetaRow({ label, value }: { label: string; value: ReactNode }) {
  return (
    <>
      <dt style={S.metaKey}>{label}</dt>
      <dd style={S.metaVal}>{value}</dd>
    </>
  );
}

function tryPrettyJson(s: string): string {
  try { return JSON.stringify(JSON.parse(s), null, 2); } catch { return s; }
}

function highlightText(text: string, query: string): ReactNode {
  if (!query) return text;
  const escaped = query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const parts = text.split(new RegExp(`(${escaped})`, 'gi'));
  if (parts.length <= 1) return text;
  return parts.map((part, i) =>
    i % 2 === 1 ? <mark key={i} style={S.highlight}>{part}</mark> : part
  );
}

const S: Record<string, CSSProperties> = {
  placeholder: {
    flex: 1,
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 6,
  },
  placeholderTitle: {
    fontSize: 13,
    fontWeight: 500,
    letterSpacing: '-0.005em',
    color: C.t3,
  },
  placeholderHint: {
    fontSize: 10,
    fontFamily: 'var(--font-mono)',
    letterSpacing: '0.04em',
    color: C.t4,
  },
  col: {
    flex: 1,
    minWidth: 0,
    minHeight: 0,
    display: 'flex',
    flexDirection: 'column',
    overflow: 'hidden',
    background: C.card,
  },
  header: {
    flexShrink: 0,
    padding: 'var(--sp-md) var(--sp-xl)',
    borderBottom: `1px solid ${C.border}`,
    background: C.card,
  },
  scrollArea: {
    flex: 1,
    minHeight: 0,
    overflowY: 'auto',
    padding: 'var(--sp-xl)',
  },
  footer: {
    flexShrink: 0,
    padding: 'var(--sp-md) var(--sp-xl) var(--sp-lg)',
    borderTop: `1px solid ${C.border}`,
    background: C.card,
    display: 'flex',
    flexDirection: 'column',
    gap: 'var(--sp-md)',
  },
  stamp: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    fontSize: 10,
    fontFamily: 'var(--font-mono)',
    letterSpacing: '0.02em',
    color: C.t3,
  },
  code: {
    background: C.card2,
    border: `1px solid ${C.border}`,
    borderRadius: 6,
    padding: 'var(--sp-lg)',
    fontFamily: 'var(--font-mono)',
    fontSize: 13,
    lineHeight: 1.6,
    color: C.t1,
    whiteSpace: 'pre-wrap',
    wordBreak: 'break-word',
    margin: 0,
  },
  prose: {
    fontSize: 14.5,
    lineHeight: 1.65,
    letterSpacing: '-0.005em',
    color: C.t1,
    whiteSpace: 'pre-wrap',
    wordBreak: 'break-word',
    maxWidth: '68ch',
    margin: 0,
  },
  imageStage: {
    flex: 1,
    minHeight: 0,
    minWidth: 0,
    overflow: 'hidden',
    padding: 'var(--sp-xl)',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'center',
  },
  imageFit: {
    display: 'block',
    maxWidth: '100%',
    maxHeight: '100%',
    width: 'auto',
    height: 'auto',
    objectFit: 'contain',
    borderRadius: 2,
  },
  colorStage: {
    flex: 1,
    minHeight: 0,
    minWidth: 0,
    overflow: 'hidden',
    padding: 'var(--sp-xl)',
    display: 'flex',
    flexDirection: 'column',
    alignItems: 'center',
    justifyContent: 'center',
    gap: 'var(--sp-md)',
  },
  colorSwatchLarge: {
    width: 96,
    height: 96,
    borderRadius: '50%',
    border: `1px solid ${C.border}`,
    boxShadow: `0 0 0 10px ${C.card2}`,
    flexShrink: 0,
  },
  colorValue: {
    maxWidth: '100%',
    fontFamily: 'var(--font-mono)',
    fontSize: 13,
    lineHeight: 1.4,
    color: C.t1,
    wordBreak: 'break-word',
    textAlign: 'center',
  },
  actions: { display: 'flex', gap: 'var(--sp-sm)', alignItems: 'center' },
  btnPrimary: {
    padding: '6px var(--sp-md)',
    background: C.accent,
    color: C.accentOn,
    border: 'none',
    borderRadius: 5,
    fontFamily: 'inherit',
    fontSize: 12,
    cursor: 'pointer',
    display: 'inline-flex',
    alignItems: 'center',
    gap: 6,
  },
  btnGhost: {
    padding: '6px var(--sp-md)',
    background: 'transparent',
    color: C.t2,
    border: `1px solid ${C.border}`,
    borderRadius: 5,
    fontFamily: 'inherit',
    fontSize: 12,
    cursor: 'pointer',
    display: 'inline-flex',
    alignItems: 'center',
    gap: 6,
  },
  kbdHint: {
    fontFamily: 'var(--font-mono)',
    fontSize: 10,
    opacity: 0.6,
    letterSpacing: '0.04em',
  },
  metaList: {
    margin: 0,
    display: 'grid',
    gridTemplateColumns: '80px 1fr',
    rowGap: 5,
    columnGap: 12,
    fontFamily: 'var(--font-mono)',
    fontSize: 11,
  },
  metaKey: {
    color: C.t3,
    letterSpacing: '0.01em',
    margin: 0,
  },
  metaVal: { color: C.t1, margin: 0, wordBreak: 'break-all' },
  appMeta: {
    display: 'inline-flex',
    alignItems: 'center',
    gap: 6,
    minWidth: 0,
  },
  appIcon: {
    width: 14,
    height: 14,
    borderRadius: 3,
    flexShrink: 0,
  },
  highlight: {
    background: 'var(--highlight)',
    borderRadius: 2,
    padding: '0 1px',
    color: 'inherit',
  },
};
