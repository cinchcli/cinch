import type { ReactElement } from "react";
import { C } from "../design";
import { IconClipboard, IconSearch } from "../icons";

interface EmptyStateProps {
  variant: "no-clips" | "search-miss" | "no-remote";
  query?: string;
}

export function EmptyState({ variant, query }: EmptyStateProps) {
  const headingMap: Record<string, string> = {
    "no-clips": "Copy anything to start.",
    "search-miss": `No clips match \u201C${query}\u201D.`,
    "no-remote": "No remote clips yet.",
  };

  const bodyMap: Record<string, string> = {
    "no-clips": "Clips appear here instantly.",
    "search-miss": "Try a shorter word or clear the search.",
    "no-remote": "Push from another machine to see clips here.",
  };

  const iconMap: Record<string, ReactElement> = {
    "no-clips": <IconClipboard size={32} />,
    "search-miss": <IconSearch size={24} />,
    "no-remote": <IconClipboard size={32} />,
  };

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        flex: 1,
        padding: "32px 16px",
        textAlign: "center",
      }}
    >
      <div style={{ color: C.t3, marginBottom: 24 }}>
        {iconMap[variant]}
      </div>

      <div
        data-testid="empty-heading"
        style={{
          fontSize: 19,
          fontWeight: 600,
          color: C.t1,
          textWrap: "balance",
          lineHeight: 1.3,
          letterSpacing: '-0.014em',
        }}
      >
        {headingMap[variant]}
      </div>

      <div
        data-testid="empty-body"
        style={{
          fontSize: 14,
          fontWeight: 500,
          color: C.t2,
          textWrap: "balance",
          lineHeight: 1.5,
          letterSpacing: "-0.005em",
          marginTop: 8,
        }}
      >
        {bodyMap[variant]}
      </div>
    </div>
  );
}
