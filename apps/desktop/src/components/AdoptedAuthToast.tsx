import { useEffect, useState } from "react";
import { events } from "../bindings";
import { C } from "../design";

/// One-shot toast that fires when the FS watcher detects credentials written
/// by the CLI on the same Mac. Tells the user "you're signed in — we noticed
/// you authenticated in your terminal" without requiring a second sign-in.
export function AdoptedAuthToast() {
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const unsubP = events.authAdoptedFromCli.listen((e) => {
      const userShort = e.payload.user_short || "your account";
      setMessage(`Signed in as ${userShort}. Auth was detected from your terminal.`);
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => setMessage(null), 4000);
    });
    return () => {
      if (timer) clearTimeout(timer);
      unsubP.then((f) => f());
    };
  }, []);

  if (!message) return null;
  return (
    <div
      style={{
        position: "fixed",
        top: 14,
        right: 14,
        background: C.card2,
        border: `1px solid ${C.border}`,
        borderRadius: 8,
        padding: "8px 14px",
        fontSize: 12,
        color: C.t2,
        zIndex: 250,
        boxShadow: "0 6px 20px rgba(0,0,0,0.3)",
        maxWidth: 320,
      }}
    >
      {message}
    </div>
  );
}
