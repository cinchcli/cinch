// Inline SVG icons (Phosphor-inspired, 1.75 stroke, currentColor).
// Replaces emoji per DESIGN.md.
import type { CSSProperties, SVGProps, ReactElement } from "react";

type IconProps = { size?: number; style?: CSSProperties };

const base = (size: number): SVGProps<SVGSVGElement> => ({
  width: size,
  height: size,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.75,
  strokeLinecap: "round",
  strokeLinejoin: "round",
});

// ─── UI Icons ─────────────────────────────────────────────

export const IconSearch = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <circle cx="11" cy="11" r="7" />
    <path d="m20 20-3.5-3.5" />
  </svg>
);

export const IconX = ({ size = 12, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <path d="M6 6l12 12M18 6L6 18" />
  </svg>
);

export const IconCopy = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <rect x="9" y="9" width="11" height="11" rx="2" />
    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
  </svg>
);

export const IconTrash = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <path d="M4 7h16M9 7V4h6v3M6 7l1 13a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-13" />
  </svg>
);

export const IconDoc = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9l-6-6Z" />
    <path d="M14 3v6h6" />
  </svg>
);

export const IconCode = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <path d="m8 8-5 4 5 4M16 8l5 4-5 4M14 4l-4 16" />
  </svg>
);

export const IconLink = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <path d="M10 13a5 5 0 0 0 7.07 0l3-3a5 5 0 0 0-7.07-7.07l-1.5 1.5M14 11a5 5 0 0 0-7.07 0l-3 3a5 5 0 0 0 7.07 7.07l1.5-1.5" />
  </svg>
);

export const IconImage = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <rect x="3" y="3" width="18" height="18" rx="2" />
    <circle cx="9" cy="9" r="2" />
    <path d="m21 15-4-4L5 21" />
  </svg>
);

// ─── Trust / disclosure icons ─────────────────────────────

export const IconLock = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <rect x="3" y="11" width="18" height="11" rx="2" />
    <path d="M7 11V7a5 5 0 0 1 10 0v4" />
  </svg>
);

export const IconEye = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7Z" />
    <circle cx="12" cy="12" r="3" />
  </svg>
);

export const IconArrowRight = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <path d="M5 12h14M13 6l6 6-6 6" />
  </svg>
);

export const IconCheck = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} strokeWidth={2.4} style={style}>
    <path d="M20 6 9 17l-5-5" />
  </svg>
);

// ─── Theme toggle icons ───────────────────────────────────

export const IconSun = ({ size = 13, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41" />
  </svg>
);

export const IconMoon = ({ size = 13, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
  </svg>
);

export const IconGear = ({ size = 13, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.01a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.01a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
  </svg>
);

// ─── Brand mark ───────────────────────────────────────────
// Cinch logo: open-arc "C" + small rect (pipe). Single stroke + fill via
// currentColor so the parent's color token drives dark/light theming.
export const IconCinch = ({ size = 22, style }: IconProps) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 180 180"
    fill="none"
    xmlns="http://www.w3.org/2000/svg"
    style={style}
    aria-hidden="true"
  >
    <path
      d="M130.154 60.9231C123.825 50.6639 114.323 42.7493 103.088 38.3807C91.8538 34.0121 79.5007 33.4279 67.904 36.7169C56.3073 40.0059 46.1003 46.9884 38.832 56.6047C31.5638 66.221 27.6313 77.946 27.6313 90C27.6313 102.054 31.5638 113.779 38.832 123.395C46.1003 133.012 56.3073 139.994 67.904 143.283C79.5007 146.572 91.8538 145.988 103.088 141.619C114.323 137.251 123.825 129.336 130.154 119.077"
      stroke="currentColor"
      strokeWidth="30.4615"
    />
    <rect
      x="132.923"
      y="67.8462"
      width="20.7692"
      height="44.3077"
      rx="2.07692"
      fill="currentColor"
    />
  </svg>
);

// ─── Content type icons ──────────────────────────────────

export const IconBinary = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9l-6-6Z" />
    <path d="M14 3v6h6" />
    <path d="M9 13h2M13 13h2" />
    <path d="M9 17h2M13 17h2" />
  </svg>
);

export const IconClipboard = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <rect x="8" y="2" width="8" height="4" rx="1" />
    <path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2" />
  </svg>
);

export const IconPin = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <line x1="12" y1="17" x2="12" y2="22" />
    <path d="M5 17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V6h1a2 2 0 0 0 0-4H8a2 2 0 0 0 0 4h1v4.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24Z" />
  </svg>
);

export const IconInbox = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <path d="M22 13h-7l-2 3h-2l-2-3H2" />
    <path d="M5.45 5.11L2 13v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-7.89A2 2 0 0 0 16.7 4H7.3a2 2 0 0 0-1.85 1.11Z" />
  </svg>
);

export const IconMonitor = ({ size = 14, style }: IconProps) => (
  <svg {...base(size)} style={style}>
    <rect x="2" y="3" width="20" height="14" rx="2" />
    <line x1="8" y1="21" x2="16" y2="21" />
    <line x1="12" y1="17" x2="12" y2="21" />
  </svg>
);

// ─── Type glyph map ───────────────────────────────────────

export function typeGlyph(t: string, size = 14): ReactElement {
  switch (t) {
    case "code":  return <IconCode size={size} />;
    case "url":   return <IconLink size={size} />;
    case "image": return <IconImage size={size} />;
    default:      return <IconDoc size={size} />;
  }
}
