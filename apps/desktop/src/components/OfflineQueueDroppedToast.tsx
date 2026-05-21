import { useEffect, useState } from "react";
import { events } from "../bindings";
import { C } from "../design";

export function OfflineQueueDroppedToast() {
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | null = null;
    const unsubP = events.offlineQueueDropped.listen((e) => {
      const count = e.payload.count;
      setMessage(
        `${count} clip${count !== 1 ? "s" : ""} were dropped because your encryption key is missing. Sign in again to restore your key.`
      );
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => setMessage(null), 6000);
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
