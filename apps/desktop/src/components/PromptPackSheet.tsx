import { useEffect, useMemo, useRef, useState, type CSSProperties } from 'react';
import type { LocalClip, PromptRecipeDto } from '../bindings';
import { C } from '../design';
import { IconSearch, IconX } from '../icons';
import { dialogStyles } from './dialogPrimitives';

interface PromptPackSheetProps {
  primaryClip: LocalClip;
  clips: LocalClip[];
  recipes: PromptRecipeDto[];
  onBuild: (recipeId: string, contextClipIds: string[]) => void;
  onClose: () => void;
}

export function PromptPackSheet({
  primaryClip,
  clips,
  recipes,
  onBuild,
  onClose,
}: PromptPackSheetProps) {
  const [query, setQuery] = useState('');
  const [highlight, setHighlight] = useState(0);
  const [contextIds, setContextIds] = useState<string[]>([]);
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    inputRef.current?.focus();
    inputRef.current?.select();
  }, []);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);

  const filteredRecipes = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return recipes;
    return recipes.filter((recipe) =>
      recipe.label.toLowerCase().includes(q) ||
      recipe.id.toLowerCase().includes(q) ||
      recipe.description.toLowerCase().includes(q)
    );
  }, [recipes, query]);

  const contextCandidates = useMemo(() => {
    return clips
      .filter((clip) => clip.id !== primaryClip.id && clip.content_type !== 'image' && clip.content.trim())
      .slice(0, 8);
  }, [clips, primaryClip.id]);

  useEffect(() => {
    setHighlight(0);
  }, [query, recipes.length]);

  useEffect(() => {
    if (filteredRecipes.length === 0) {
      setHighlight(0);
      return;
    }
    setHighlight((current) => Math.min(current, filteredRecipes.length - 1));
  }, [filteredRecipes.length]);

  const selected = filteredRecipes[highlight] ?? null;

  const toggleContext = (clipId: string) => {
    setContextIds((current) =>
      current.includes(clipId)
        ? current.filter((id) => id !== clipId)
        : [...current, clipId]
    );
  };

  return (
    <div role="presentation" style={S.overlay} onClick={onClose}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Prompt Pack"
        style={S.dialog}
        onClick={(e) => e.stopPropagation()}
      >
        <div style={S.header}>
          <div style={S.titleBlock}>
            <div style={S.title}>Prompt Pack</div>
            <div style={S.subtitle}>Use the latest clip, add context, copy an AI-ready prompt.</div>
          </div>
          <button type="button" aria-label="Close" style={S.closeBtn} onClick={onClose}>
            <IconX size={12} />
          </button>
        </div>

        <div style={S.primaryBlock}>
          <div style={S.sectionLabel}>Primary clip</div>
          <div style={S.primaryPreview}>{previewText(primaryClip.content)}</div>
        </div>

        {contextCandidates.length > 0 && (
          <div style={S.contextBlock}>
            <div style={S.sectionLabel}>Add context</div>
            <div style={S.contextList}>
              {contextCandidates.map((clip) => (
                <label key={clip.id} style={S.contextRow}>
                  <input
                    type="checkbox"
                    checked={contextIds.includes(clip.id)}
                    onChange={() => toggleContext(clip.id)}
                    style={S.checkbox}
                  />
                  <span style={S.contextText}>{previewText(clip.content)}</span>
                </label>
              ))}
            </div>
          </div>
        )}

        <label style={S.searchRow}>
          <span style={S.searchIcon}><IconSearch size={13} /></span>
          <input
            ref={inputRef}
            aria-label="Prompt Pack"
            value={query}
            onChange={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                e.preventDefault();
                onClose();
                return;
              }
              if (e.key === 'ArrowDown') {
                e.preventDefault();
                if (filteredRecipes.length === 0) return;
                setHighlight((current) => Math.min(current + 1, filteredRecipes.length - 1));
                return;
              }
              if (e.key === 'ArrowUp') {
                e.preventDefault();
                if (filteredRecipes.length === 0) return;
                setHighlight((current) => Math.max(current - 1, 0));
                return;
              }
              if (e.key === 'Enter') {
                e.preventDefault();
                if (selected) onBuild(selected.id, contextIds);
              }
            }}
            placeholder="Choose recipe"
            style={S.input}
          />
        </label>

        <div style={S.recipeList} role="listbox" aria-label="Prompt Pack recipes">
          {filteredRecipes.length === 0 ? (
            <div style={S.empty}>No matching recipes.</div>
          ) : (
            filteredRecipes.map((recipe, index) => (
              <button
                key={recipe.id}
                type="button"
                role="option"
                aria-selected={index === highlight}
                onMouseEnter={() => setHighlight(index)}
                onClick={() => onBuild(recipe.id, contextIds)}
                style={{ ...S.recipeRow, ...(index === highlight ? S.recipeRowActive : null) }}
              >
                <span style={S.recipeLabel}>{recipe.label}</span>
                <span style={S.recipeDescription}>{recipe.description}</span>
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  );
}

function previewText(text: string): string {
  const normalized = text.replace(/\s+/g, ' ').trim();
  if (normalized.length <= 96) return normalized;
  return `${normalized.slice(0, 95)}...`;
}

const S: Record<string, CSSProperties> = {
  overlay: {
    position: 'fixed',
    inset: 0,
    zIndex: 250,
    background: 'rgba(0, 0, 0, 0.38)',
    display: 'flex',
    justifyContent: 'center',
    alignItems: 'flex-start',
    paddingTop: 52,
  },
  dialog: {
    ...dialogStyles.dialog,
    width: 'min(540px, calc(100vw - 32px))',
    maxWidth: 'min(540px, calc(100vw - 32px))',
    padding: 0,
    overflow: 'hidden',
  },
  header: {
    display: 'flex',
    alignItems: 'flex-start',
    justifyContent: 'space-between',
    gap: 12,
    padding: '18px 18px 14px',
    borderBottom: `1px solid ${C.border}`,
  },
  titleBlock: {
    minWidth: 0,
    display: 'flex',
    flexDirection: 'column',
    gap: 2,
  },
  title: {
    fontSize: 13,
    fontWeight: 600,
    color: C.t1,
    lineHeight: 1.2,
  },
  subtitle: {
    fontSize: 12,
    lineHeight: 1.4,
    color: C.t2,
  },
  closeBtn: {
    border: `1px solid ${C.border}`,
    background: C.card,
    color: C.t2,
    width: 24,
    height: 24,
    borderRadius: 6,
    display: 'grid',
    placeItems: 'center',
    cursor: 'pointer',
    flexShrink: 0,
  },
  primaryBlock: {
    padding: '12px 18px',
    borderBottom: `1px solid ${C.border}`,
    display: 'flex',
    flexDirection: 'column',
    gap: 6,
  },
  sectionLabel: {
    fontSize: 11,
    color: C.t3,
    fontWeight: 600,
    textTransform: 'uppercase',
  },
  primaryPreview: {
    fontSize: 12,
    lineHeight: 1.45,
    color: C.t1,
    fontFamily: 'var(--font-mono)',
    wordBreak: 'break-word',
  },
  contextBlock: {
    padding: '12px 18px',
    borderBottom: `1px solid ${C.border}`,
    display: 'flex',
    flexDirection: 'column',
    gap: 8,
  },
  contextList: {
    display: 'flex',
    flexDirection: 'column',
    gap: 4,
    maxHeight: 124,
    overflowY: 'auto',
  },
  contextRow: {
    display: 'flex',
    alignItems: 'flex-start',
    gap: 8,
    borderRadius: 6,
    padding: '6px 4px',
    cursor: 'pointer',
  },
  checkbox: {
    flexShrink: 0,
    marginTop: 2,
  },
  contextText: {
    fontSize: 12,
    lineHeight: 1.35,
    color: C.t2,
    minWidth: 0,
  },
  searchRow: {
    display: 'flex',
    alignItems: 'center',
    gap: 8,
    padding: '12px 18px',
    borderBottom: `1px solid ${C.border}`,
  },
  searchIcon: {
    color: C.t3,
    display: 'inline-flex',
    alignItems: 'center',
    justifyContent: 'center',
    flexShrink: 0,
  },
  input: {
    width: '100%',
    border: 'none',
    outline: 'none',
    background: 'transparent',
    color: C.t1,
    fontSize: 14,
    fontFamily: 'inherit',
    minWidth: 0,
  },
  recipeList: {
    maxHeight: 260,
    overflowY: 'auto',
    padding: 6,
  },
  recipeRow: {
    width: '100%',
    border: 'none',
    background: 'transparent',
    color: C.t1,
    borderRadius: 6,
    cursor: 'pointer',
    display: 'flex',
    flexDirection: 'column',
    gap: 3,
    padding: '9px 10px',
    textAlign: 'left',
  },
  recipeRowActive: {
    background: C.hover,
  },
  recipeLabel: {
    fontSize: 13,
    fontWeight: 600,
    color: C.t1,
  },
  recipeDescription: {
    fontSize: 12,
    color: C.t3,
    lineHeight: 1.35,
  },
  empty: {
    padding: '18px 10px 20px',
    fontSize: 13,
    color: C.t3,
  },
};
