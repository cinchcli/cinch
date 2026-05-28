import { useState, type CSSProperties } from "react";
import type { LocalClip } from "../bindings";
import { C, formatTime, formatBytes } from "../design";
import { IconCopy, IconTrash, typeGlyph, IconBinary } from "../icons";
import { parseColorClip } from "../lib/colorClip";

// ─── Props ────────────────────────────────────────────────

interface ClipCardProps {
  clip: LocalClip;
  selected: boolean;
  onCopy: () => void;
  onDelete: () => void;
  onClick: () => void;
  onDoubleClick?: () => void;
}

// ─── Helpers ──────────────────────────────────────────────

function isTextType(ct: string): boolean {
  return ct === "text" || ct === "code" || ct === "url";
}

function isImageType(clip: LocalClip): boolean {
  return clip.content_type === "image";
}

// ─── ClipCard ─────────────────────────────────────────────────

export function ClipCard({ clip, selected, onCopy, onDelete, onClick, onDoubleClick }: ClipCardProps) {
  const [hover, setHover] = useState(false);
  const [imgError, setImgError] = useState(false);

  const isImage = isImageType(clip);
  const isText = isTextType(clip.content_type);
  const isBinary = !isText && !isImage;
  const colorClip = clip.content_type === "text" ? parseColorClip(clip.content) : null;

  const containerStyle: CSSProperties = {
    display: "flex",
    alignItems: "center",
    gap: 10,
    padding: "12px 16px",
    background: selected ? C.selected : hover ? C.hover : C.card,
    border: `1px solid ${hover ? C.borderHover : C.border}`,
    borderRadius: 8,
    cursor: "pointer",
    transition: "border-color 150ms ease, background-color 150ms ease",
    boxShadow: selected ? `inset 2px 0 0 var(--selection-bar)` : undefined,
    position: "relative",
  };

  return (
    <div
      role="button"
      data-id={clip.id}
      aria-selected={selected}
      tabIndex={0}
      style={containerStyle}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      onClick={onClick}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') {
          e.preventDefault();
          onClick();
        }
      }}
      onDoubleClick={onDoubleClick}
    >
      {/* Left column: type glyph or thumbnail */}
      {isText && colorClip && (
        <div
          data-testid="color-swatch"
          aria-label={`Color preview for ${colorClip.value}`}
          style={{
            width: 32,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
          }}
        >
          <span
            style={{
              width: 22,
              height: 22,
              borderRadius: "50%",
              backgroundColor: colorClip.cssColor,
              border: `1px solid ${C.borderHover}`,
              boxShadow: `0 0 0 3px ${C.card2}`,
            }}
          />
        </div>
      )}

      {isText && !colorClip && (
        <div
          data-testid="type-glyph"
          style={{
            width: 32,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            color: C.t2,
            flexShrink: 0,
          }}
        >
          {typeGlyph(clip.content_type, 14)}
        </div>
      )}

      {isImage && !imgError && (
        <img
          src={`cinch://media/${clip.id}`}
          alt=""
          style={{
            width: 48,
            height: 48,
            objectFit: "cover",
            borderRadius: 6,
            border: `1px solid ${C.border}`,
            flexShrink: 0,
          }}
          onError={() => setImgError(true)}
        />
      )}

      {isImage && imgError && (
        <div
          style={{
            width: 48,
            height: 48,
            borderRadius: 6,
            background: C.card2,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
            backgroundColor: "rgba(255,99,99,0.15)",
          }}
        >
          <span style={{ fontSize: 12, color: C.t2, textAlign: "center" }}>
            Preview unavailable
          </span>
        </div>
      )}

      {isBinary && (
        <div
          data-testid="binary-slot"
          style={{
            width: 48,
            height: 48,
            borderRadius: 6,
            background: C.card2,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            flexShrink: 0,
          }}
        >
          <IconBinary size={20} style={{ color: C.t2 }} />
        </div>
      )}

      {/* Content column */}
      <div style={{ flex: 1, minWidth: 0 }}>
        {isText && (
          <div
            style={{
              fontFamily: 'var(--font-body)',
              fontSize: 16,
              fontWeight: 500,
              color: C.t1,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
              lineHeight: 1.15,
              letterSpacing: "0.1px",
            }}
          >
            {clip.content.trim().replace(/\s+/g, " ").slice(0, 140)}
          </div>
        )}

        {isImage && (
          <>
            <div style={{
              fontFamily: 'var(--font-body)',
              fontSize: 14,
              fontWeight: 500,
              color: C.t1,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}>
              Image ({formatBytes(clip.byte_size)})
            </div>
            <div style={{
              fontFamily: 'var(--font-body)',
              fontSize: 12,
              fontWeight: 600,
              color: C.t2,
            }}>
              {clip.content_type}
            </div>
          </>
        )}

        {isBinary && (
          <>
            <div style={{
              fontFamily: 'var(--font-body)',
              fontSize: 14,
              fontWeight: 500,
              color: C.t1,
              whiteSpace: "nowrap",
              overflow: "hidden",
              textOverflow: "ellipsis",
            }}>
              {clip.label || clip.content_type}
            </div>
            <div style={{
              fontFamily: 'var(--font-mono)',
              fontSize: 12,
              fontWeight: 600,
              color: C.t2,
              fontVariantNumeric: "tabular-nums",
            }}>
              {formatBytes(clip.byte_size)}
            </div>
          </>
        )}
      </div>

      {/* Right column: timestamp + key-cap + actions */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, flexShrink: 0 }}>
        <span
          data-testid="timestamp"
          style={{
            fontFamily: 'var(--font-mono)',
            fontSize: 12,
            fontWeight: 600,
            color: C.t2,
            fontVariantNumeric: "tabular-nums",
          }}
        >
          {formatTime(clip.created_at)}
        </span>

        {/* Action buttons — hover-revealed */}
        <div style={{
          display: "flex",
          gap: 4,
          opacity: hover ? 1 : 0,
          transition: "opacity 150ms ease",
        }}>
          <button
            aria-label="Copy clip"
            onClick={(e) => { e.stopPropagation(); onCopy(); }}
            style={{
              background: "none",
              border: "none",
              cursor: "pointer",
              padding: 8,
              borderRadius: 4,
              color: C.t3,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              width: 28,
              height: 28,
            }}
          >
            <IconCopy size={14} />
          </button>
          <button
            aria-label="Delete clip"
            onClick={(e) => { e.stopPropagation(); onDelete(); }}
            style={{
              background: "none",
              border: "none",
              cursor: "pointer",
              padding: 8,
              borderRadius: 4,
              color: C.t3,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              width: 28,
              height: 28,
            }}
          >
            <IconTrash size={14} />
          </button>
        </div>
      </div>
    </div>
  );
}
