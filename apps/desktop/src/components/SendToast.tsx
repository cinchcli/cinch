import { useEffect, useState } from "react";
import { events } from "../bindings";
import { C } from "../design";

export function SendToast() {
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const unsubP = events.clipSent.listen((e) => {
      setMessage(e.payload ? "Sent to your devices" : "Nothing to send");
      if (timer) clearTimeout(timer);
      // Short auto-dismiss: a success acknowledgement, not an error to read
      // (error toasts use 6000ms). Matches App.tsx's inline Toast (1800ms).
      timer = setTimeout(() => setMessage(null), 1800);
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
