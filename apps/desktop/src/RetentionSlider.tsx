// RetentionSlider — plan 01-07 UI-SPEC §RetentionSlider.
// Discrete-step slider (7/14/30/60/90 days) with commit-on-release.
// Webkit thumb + track styling lives in App.css `.retention-slider` scope
// (focus-RING semantics — 4px spread, zero blur per UI-SPEC §Color Glow).

import { useEffect, useId, useRef, useState, type CSSProperties } from "react";
import { C } from "./design";

const STEPS = [7, 14, 30, 60, 90] as const;
type Step = (typeof STEPS)[number];

interface RetentionSliderProps {
  label: string;
  description: string;
  value: number;
  onCommit: (next: number) => Promise<void> | void;
  id: string;
}

const NARROW_BREAKPOINT_PX = 480;

export default function RetentionSlider({
  label,
  description,
  value,
  onCommit,
  id,
}: RetentionSliderProps) {
  const labelId = useId();
  const descId = `${id}-desc`;
  const containerRef = useRef<HTMLDivElement | null>(null);

  // Snap incoming value to STEPS; fall back to index 0 if parent passed a
  // non-canonical value (shouldn't happen — defense in depth).
  const stepIndex = Math.max(
    0,
    STEPS.indexOf(value as Step)
  );

  // liveIndex tracks drag position without committing. Parent-driven updates
  // sync via the useEffect below.
  const [liveIndex, setLiveIndex] = useState(stepIndex);
  useEffect(() => setLiveIndex(stepIndex), [stepIndex]);

  // Hide tick labels when the container is narrower than 480px.
  // ResizeObserver catches container resize even when viewport is unchanged.
  const [showTicks, setShowTicks] = useState(true);
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setShowTicks(entry.contentRect.width >= NARROW_BREAKPOINT_PX);
      }
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const styles: Record<string, CSSProperties> = {
    wrapper: {
      display: "flex",
      flexDirection: "column",
      gap: 12,
    },
    label: {
      fontSize: 16,
      fontWeight: 600,
      letterSpacing: "0.2px",
      color: C.t1,
    },
    description: {
      fontSize: 16,
      fontWeight: 500,
      lineHeight: 1.6,
      color: C.t2,
    },
    row: {
      display: "flex",
      alignItems: "center",
      gap: 16,
    },
    readout: {
      fontSize: 14,
      fontWeight: 500,
      color: C.t1,
      fontVariantNumeric: "tabular-nums" as const,
      minWidth: 64,
      textAlign: "right" as const,
    },
    ticks: {
      display: "flex",
      justifyContent: "space-between",
      marginTop: 2,
    },
    tick: {
      fontSize: 14,
      fontWeight: 500,
      color: C.t3,
      fontVariantNumeric: "tabular-nums" as const,
    },
  };

  return (
    <div
      ref={containerRef}
      className="retention-slider"
      style={styles.wrapper}
    >
      <label id={labelId} htmlFor={id} style={styles.label}>
        {label}
      </label>
      <div id={descId} style={styles.description}>
        {description}
      </div>
      <div style={styles.row}>
        <input
          id={id}
          type="range"
          min={0}
          max={STEPS.length - 1}
          step={1}
          value={liveIndex}
          onInput={(e) =>
            setLiveIndex(Number((e.currentTarget as HTMLInputElement).value))
          }
          onChange={(e) =>
            setLiveIndex(Number((e.currentTarget as HTMLInputElement).value))
          }
          onPointerUp={(e) => {
            const next = Number((e.currentTarget as HTMLInputElement).value);
            if (STEPS[next] !== value) void onCommit(STEPS[next]);
          }}
          onKeyUp={(e) => {
            const el = e.currentTarget as HTMLInputElement;
            const keys = [
              "ArrowLeft",
              "ArrowRight",
              "ArrowUp",
              "ArrowDown",
              "Home",
              "End",
              "PageUp",
              "PageDown",
            ];
            if (!keys.includes(e.key)) return;
            const next = Number(el.value);
            if (STEPS[next] !== value) void onCommit(STEPS[next]);
          }}
          aria-labelledby={labelId}
          aria-describedby={descId}
          aria-valuemin={0}
          aria-valuemax={STEPS.length - 1}
          aria-valuenow={liveIndex}
          aria-valuetext={`${STEPS[liveIndex]} days`}
        />
        <span style={styles.readout}>{STEPS[liveIndex]} days</span>
      </div>
      {showTicks && (
        <div style={styles.ticks} aria-hidden="true">
          {STEPS.map((s) => (
            <span key={s} style={styles.tick}>
              {s}d
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
