import { useState } from "react";
import { C } from "../design";
import { IconX } from "../icons";
import { AddRelayDialog } from "./AddRelayDialog";

interface UpgradePromptProps {
  onDismiss: () => void;
}

export function UpgradePrompt({ onDismiss }: UpgradePromptProps) {
  const [hover, setHover] = useState(false);
  const [addRelayOpen, setAddRelayOpen] = useState(false);

  const handleSignIn = () => setAddRelayOpen(true);

  return (
    <>
    <div
      style={{
        width: "100%",
        background: C.card,
        borderTop: `1px solid ${C.border}`,
        height: 44,
        display: "flex",
        alignItems: "center",
        justifyContent: "space-between",
        padding: "0 16px",
        flexShrink: 0,
      }}
    >
      <span
        style={{
          fontFamily: 'var(--font-body)',
          fontSize: 14,
          fontWeight: 500,
          color: C.t2,
          letterSpacing: "0.2px",
        }}
      >
        <button
          onClick={handleSignIn}
          onMouseEnter={() => setHover(true)}
          onMouseLeave={() => setHover(false)}
          style={{
            background: "none",
            border: "none",
            cursor: "pointer",
            fontFamily: "inherit",
            fontSize: "inherit",
            fontWeight: "inherit",
            letterSpacing: "inherit",
            color: hover ? C.t2 : C.t1,
            textDecoration: "underline",
            textUnderlineOffset: "2px",
            padding: 0,
            transition: "color 150ms ease",
          }}
        >
          Sign in
        </button>
        {" "}for cross-machine sync
      </span>

      <button
        aria-label="Dismiss upgrade prompt"
        onClick={onDismiss}
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
        <IconX size={12} />
      </button>
    </div>
    {addRelayOpen && (
      <AddRelayDialog onClose={() => setAddRelayOpen(false)} />
    )}
    </>
  );
}
