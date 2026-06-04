import { useEffect, useState, type CSSProperties } from "react";
import { events } from "./bindings";

type Guide = {
  // logical (CSS) px, relative to the overlay window (which covers the monitor)
  leftX: number;
  rightX: number;
  topY: number;
  snapX: boolean;
  snapY: boolean;
  distX: number;
  distY: number;
  visible: boolean;
};

function getOpacity(dist: number, isSnapped: boolean): number {
  if (isSnapped) return 1.0;
  const maxInactive = 0.5;
  const minInactive = 0.05;
  const range = 250; // Fade out completely over 250px
  const normalized = Math.max(0, Math.min(1, dist / range));
  return maxInactive - (maxInactive - minInactive) * normalized;
}

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
      const topY = (p.anchor_y - p.monitor_y) / s;
      setG({
        leftX,
        rightX,
        topY,
        snapX: p.snap_x,
        snapY: p.snap_y,
        distX: p.dist_x,
        distY: p.dist_y,
        visible: true,
      });
    });
    return () => {
      unsub.then((f) => f());
    };
  }, []);

  if (!g || !g.visible) return null;

  // Monochrome redesign: the snapped state reads through brightness + glow, not
  // hue. Raw values (not theme tokens) because this overlay paints over the
  // desktop in a separate transparent window; near-white stays visible on most
  // wallpapers, the faint grey marks the un-snapped guide.
  const activeColor = "rgba(245,245,243,0.95)";
  const inactiveColor = "rgba(110,118,129,1)"; // Opacity handled via 'opacity' style
  const activeGlow = "0 0 12px rgba(245,245,243,0.5)";

  const vLineBase: CSSProperties = {
    position: "fixed",
    top: 0,
    bottom: 0,
    width: 0,
    transition: "border-color 150ms ease, filter 150ms ease, opacity 100ms ease",
  };

  const hLineBase: CSSProperties = {
    position: "fixed",
    left: 0,
    right: 0,
    height: 0,
    transition: "border-color 150ms ease, box-shadow 150ms ease, opacity 100ms ease",
  };

  const vOpacity = getOpacity(g.distX, g.snapX);
  const hOpacity = getOpacity(g.distY, g.snapY);

  const vColor = g.snapX ? activeColor : inactiveColor;
  const hColor = g.snapY ? activeColor : inactiveColor;

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
      {/* Vertical lines - Opacity based on distX */}
      <div
        style={{
          ...vLineBase,
          left: `${g.leftX}px`,
          borderLeft: `2px dashed ${vColor}`,
          filter: g.snapX ? `drop-shadow(0 0 4px ${activeColor})` : "none",
          opacity: vOpacity,
        }}
      />
      <div
        style={{
          ...vLineBase,
          left: `${g.rightX}px`,
          borderLeft: `2px dashed ${vColor}`,
          filter: g.snapX ? `drop-shadow(0 0 4px ${activeColor})` : "none",
          opacity: vOpacity,
        }}
      />

      {/* Horizontal line (Top only) - Opacity based on distY */}
      <div
        style={{
          ...hLineBase,
          top: `${g.topY}px`,
          borderTop: `2px dashed ${hColor}`,
          boxShadow: g.snapY ? activeGlow : "none",
          opacity: hOpacity,
        }}
      />
    </div>
  );
}
