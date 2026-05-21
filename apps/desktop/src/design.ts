// ─── Design System Tokens (DESIGN.md) ─────────────────────
// All values are CSS custom properties so dark/light themes
// are controlled purely by CSS — no JS re-render needed.

export const C = {
  bg:           "var(--bg)",
  card:         "var(--surface)",
  card2:        "var(--surface-2)",
  hover:        "var(--hover)",
  selected:     "var(--selected)",
  border:       "var(--border)",
  borderHover:  "var(--border-hover)",
  t1:           "var(--text-primary)",
  t2:           "var(--text-muted)",
  t3:           "var(--text-faint)",
  t4:           "var(--text-vfaint)",
  accent:       "var(--accent)",
  accentMuted:  "var(--accent-muted)",
  accentDim:    "var(--accent-subtle)",
  accentPastel: "var(--accent-pastel)",
  accentOn:     "var(--accent-on)",
  success:      "var(--success)",
  warning:      "var(--warning)",
  error:        "var(--error)",
  info:         "var(--info)",
} as const;

// ─── Shared Helpers ────────────────────────────────────────

export function formatTime(unix: number): string {
  const diff = Math.floor(Date.now() / 1000 - unix);
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h`;
  return `${Math.floor(diff / 86400)}d`;
}

export function formatBytes(b: number): string {
  if (b < 1024) return `${b} B`;
  if (b < 1048576) return `${(b / 1024).toFixed(1)} KB`;
  return `${(b / 1048576).toFixed(1)} MB`;
}
