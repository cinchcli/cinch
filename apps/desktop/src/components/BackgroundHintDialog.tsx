// BackgroundHintDialog — one-time "Cinch keeps running in the menu bar" hint.
//
// Shown the first time the user dismisses the window (close box / Cmd+W /
// Cmd+Q). The Rust side gates this on the `background_hint_seen` flag and emits
// `BackgroundHint` only on the first dismissal; this component renders the
// dialog and reports the user's choice via `resolveBackgroundHint(quit)`.
//
// Self-contained (subscribes to its own event) like SendToast. Reuses the
// ConfirmDialog visual recipe but with its own key/emphasis semantics: the safe
// default ("Keep in menu bar") is the prominent CTA, the initial focus, and what
// Esc / overlay-click resolve to — Quit is a quiet secondary button.

import {
  useEffect,
  useRef,
  useState,
  useCallback,
  useId,
  type CSSProperties,
} from "react";
import { commands, events } from "../bindings";
import { C } from "../design";

// DESIGN.md §6 Level 5 (dark) — same recipe as ConfirmDialog.
const DARK_SHADOW =
  "rgba(0,0,0,0.5) 0 0 0 2px, rgba(255,255,255,0.19) 0 0 14px, rgba(255,255,255,0.05) 0 1px 0 0 inset";
const PRIMARY_GLOW = "rgba(79,179,169,0.18) 0 0 20px 5px";

export function BackgroundHintDialog() {
  const [open, setOpen] = useState(false);
  const keepRef = useRef<HTMLButtonElement | null>(null);
  const titleId = useId();
  const bodyId = useId();

  // Rust emits this only on the first dismissal (flag-gated), so no local
  // "seen" bookkeeping is needed here.
  useEffect(() => {
    const unsub = events.backgroundHint.listen(() => setOpen(true));
    return () => {
      unsub.then((f) => f());
    };
  }, []);

  const resolve = useCallback((quit: boolean) => {
    setOpen(false);
    void commands.resolveBackgroundHint(quit);
  }, []);

  // Esc → keep (safe default).
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        resolve(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, resolve]);

  // Initial focus on the safe default (Keep), after the overlay paints.
  useEffect(() => {
    if (!open) return;
    const raf = requestAnimationFrame(() => keepRef.current?.focus());
    return () => cancelAnimationFrame(raf);
  }, [open]);

  if (!open) return null;

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
      background: C.t1,
      color: C.bg,
      border: "none",
      fontSize: 12,
      fontWeight: 600,
      letterSpacing: "0.3px",
      padding: "8px 14px",
      borderRadius: 6,
      cursor: "pointer",
      boxShadow: PRIMARY_GLOW,
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
    <div style={styles.overlay} onClick={() => resolve(false)} role="presentation">
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
          Cinch keeps running in the menu bar
        </h2>
        <div id={bodyId} style={styles.body}>
          Closing this window doesn&rsquo;t quit Cinch — it stays in the menu bar
          so your clipboard keeps syncing. Click the menu-bar icon (or press
          &#8984;&#8679;W) to open it again.
        </div>
        <div style={styles.actions}>
          <button
            style={styles.secondaryBtn}
            onClick={() => resolve(true)}
            type="button"
          >
            Quit Cinch
          </button>
          <button
            ref={keepRef}
            style={styles.primaryBtn}
            onClick={() => resolve(false)}
            type="button"
          >
            Keep in menu bar
          </button>
        </div>
        <div style={styles.hint}>Esc keeps Cinch in the menu bar</div>
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
