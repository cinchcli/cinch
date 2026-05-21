import { useEffect, useState, type CSSProperties } from "react";
import { events } from "./bindings";

type Guide = {
  // logical (CSS) px, relative to the overlay window (which covers the monitor)
  leftX: number;
  rightX: number;
  midY: number;
  within: boolean;
  visible: boolean;
};

export default function SnapOverlay() {
  const [g, setG] = useState<Guide | null>(null);

  useEffect(() => {
    const unsub = events.snapGuideUpdate.listen((e) => {
      const p = e.payload;
      if (!p.visible) {
        setG((prev) => (prev ? { ...prev, visible: false } : null));
        return;
      }
      const s = p.scale > 0 ? p.scale : 1;
      const leftX = (p.anchor_x - p.monitor_x) / s;
      const rightX = (p.anchor_x + p.win_w - p.monitor_x) / s;
      const midY = (p.anchor_y + p.win_h / 2 - p.monitor_y) / s;
      setG({ leftX, rightX, midY, within: p.within_snap, visible: true });
    });
    return () => {
      unsub.then((f) => f());
    };
  }, []);

  if (!g || !g.visible) return null;

  const color = g.within ? "rgba(88,166,255,0.95)" : "rgba(110,118,129,0.7)";
  const glow = g.within ? "0 0 10px rgba(88,166,255,0.6)" : "none";
  const vLine: CSSProperties = {
    position: "fixed",
    top: 0,
    bottom: 0,
    width: 0,
    borderLeft: `2px dashed ${color}`,
    filter: g.within ? "drop-shadow(0 0 4px rgba(88,166,255,0.6))" : "none",
  };
  const hLine: CSSProperties = {
    position: "fixed",
    left: 0,
    right: 0,
    height: 0,
    borderTop: `2px dashed ${color}`,
    boxShadow: glow,
  };

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "transparent",
        pointerEvents: "none",
        overflow: "hidden",
      }}
    >
      <div style={{ ...vLine, left: `${g.leftX}px` }} />
      <div style={{ ...vLine, left: `${g.rightX}px` }} />
      <div style={{ ...hLine, top: `${g.midY}px` }} />
    </div>
  );
}
