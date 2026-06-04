// ConfirmDialog — DESIGN.md §2.4 + §2.5 + §6 Level 5.
// Plan 01-07 UI-SPEC §ConfirmDialog. Does NOT reuse App.tsx's S.dialog styles
// (forbidden by PATTERNS.md — those predate the design-system polish).

import { useEffect, useRef, useId, type CSSProperties, type ReactNode } from "react";
import { C } from "./design";

type ConfirmTone = "destructive" | "primary";

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  body: ReactNode;
  primaryLabel: string;
  secondaryLabel: string;
  tone: ConfirmTone;
  onConfirm: () => void;
  onCancel: () => void;
}

// DESIGN.md §6 Level 5 (dark) — authoritative multi-stop recipe.
const DARK_SHADOW =
  "rgba(0,0,0,0.5) 0 0 0 2px, rgba(255,255,255,0.19) 0 0 14px, rgba(255,255,255,0.05) 0 1px 0 0 inset";

// DESIGN.md §2.5 flow-glow — destructive + primary tones on the primary CTA.
// Destructive glow is themed via --destructive-glow so light theme can use a
// softer halo around its vivid red button.
const DESTRUCTIVE_GLOW = "var(--destructive-glow)";
// Monochrome redesign: the non-destructive primary CTA is flat (no accent halo).
// The destructive tone keeps its red glow as a safety signal.
const PRIMARY_GLOW = "none";

export default function ConfirmDialog({
  open,
  title,
  body,
  primaryLabel,
  secondaryLabel,
  tone,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const cancelRef = useRef<HTMLButtonElement | null>(null);
  const titleId = useId();
  const bodyId = useId();

  // Esc cancels. Enter is handled by native button semantics — whichever button
  // has focus fires on Enter. Initial focus is Cancel (safe default for
  // destructive actions), so Enter without tabbing fires Cancel, not the
  // destructive action.
  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onCancel();
      }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [open, onCancel]);

  // Initial focus on Cancel (safe default for destructive actions — UI-SPEC §Interaction).
  useEffect(() => {
    if (open) {
      // requestAnimationFrame so focus lands after the overlay is painted.
      const raf = requestAnimationFrame(() => cancelRef.current?.focus());
      return () => cancelAnimationFrame(raf);
    }
  }, [open]);

  if (!open) return null;

  const isDestructive = tone === "destructive";

  const styles: Record<string, CSSProperties> = {
    overlay: {
      position: "fixed",
      inset: 0,
      background: "rgba(0,0,0,0.55)",
      zIndex: 200,
      display: "flex",
      alignItems: "center",
      justifyContent: "center",
      animation: "confirm-fade-in 200ms cubic-bezier(0.16,1,0.3,1)",
    },
    dialog: {
      background: C.card,
      border: "1px solid var(--border)",
      borderRadius: 12,
      maxWidth: 400,
      width: "calc(100% - 48px)",
      padding: "24px 24px 16px",
      color: C.t1,
      boxShadow: DARK_SHADOW,
      animation:
        "confirm-enter 250ms cubic-bezier(0.16,1,0.3,1), confirm-fade-in 200ms cubic-bezier(0.16,1,0.3,1)",
    },
    title: {
      fontSize: 20,
      fontWeight: 500,
      lineHeight: 1.6,
      letterSpacing: "0.2px",
      color: C.t1,
      marginBottom: 8,
    },
    body: {
      fontSize: 14,
      fontWeight: 500,
      lineHeight: 1.55,
      color: C.t2,
      marginBottom: 20,
    },
    actions: {
      display: "flex",
      justifyContent: "flex-end",
      gap: 8,
      marginTop: 8,
    },
    secondaryBtn: {
      background: "transparent",
      border: `1px solid ${C.borderHover}`,
      color: C.t1,
      fontSize: 12,
      fontWeight: 600,
      letterSpacing: "0.3px",
      padding: "8px 14px",
      borderRadius: 6,
      cursor: "pointer",
    },
    primaryBtn: {
      background: isDestructive ? "var(--destructive-bg)" : C.accent,
      color: isDestructive ? "var(--destructive-fg)" : C.accentOn,
      border: "none",
      fontSize: 12,
      fontWeight: 600,
      letterSpacing: "0.3px",
      padding: "8px 14px",
      borderRadius: 6,
      cursor: "pointer",
      boxShadow: isDestructive ? DESTRUCTIVE_GLOW : PRIMARY_GLOW,
    },
    hint: {
      fontSize: 12,
      fontWeight: 400,
      color: C.t3,
      marginTop: 12,
      textAlign: "left",
    },
  };

  return (
    <div
      style={styles.overlay}
      onClick={onCancel}
      role="presentation"
    >
      <div
        className="confirm-dialog"
        style={styles.dialog}
        onClick={(e) => e.stopPropagation()}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        aria-describedby={bodyId}
      >
        <h2 id={titleId} style={styles.title}>
          {title}
        </h2>
        <div id={bodyId} style={styles.body}>
          {body}
        </div>
        <div style={styles.actions}>
          <button
            ref={cancelRef}
            style={styles.secondaryBtn}
            onClick={onCancel}
            type="button"
          >
            {secondaryLabel}
          </button>
          <button
            style={styles.primaryBtn}
            onClick={onConfirm}
            type="button"
          >
            {primaryLabel}
          </button>
        </div>
        <div style={styles.hint}>Enter to confirm · Esc to cancel</div>
      </div>
      <style>{`
        @keyframes confirm-fade-in {
          from { opacity: 0; }
          to { opacity: 1; }
        }
        @keyframes confirm-enter {
          from { transform: translateY(8px); }
          to { transform: translateY(0); }
        }
        @media (prefers-reduced-motion: reduce) {
          @keyframes confirm-enter {
            from { transform: none; }
            to { transform: none; }
          }
        }
      `}</style>
    </div>
  );
}
