import { useEffect, useRef, useState, type CSSProperties } from "react";
import { events } from "./bindings";
import { IconCopy } from "./icons";

const THEME_STORAGE_KEY = "cinch-theme";

type Toast = {
  message: string;
  visible: boolean;
};

function applyStoredTheme() {
  const saved = localStorage.getItem(THEME_STORAGE_KEY);
  const prefersLight = window.matchMedia("(prefers-color-scheme: light)").matches;
  const light = saved === "light" || (saved !== "dark" && prefersLight);
  document.documentElement.classList.toggle("light", light);
}

export default function CopyToastOverlay() {
  const [toast, setToast] = useState<Toast | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    applyStoredTheme();
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const onThemeChange = () => applyStoredTheme();
    mq.addEventListener("change", onThemeChange);
    window.addEventListener("storage", onThemeChange);
    return () => {
      mq.removeEventListener("change", onThemeChange);
      window.removeEventListener("storage", onThemeChange);
    };
  }, []);

  useEffect(() => {
    const unsub = events.copyToastRequested.listen((e) => {
      if (timer.current) clearTimeout(timer.current);
      setToast({ message: e.payload.message, visible: true });
      timer.current = setTimeout(() => {
        setToast((current) => current ? { ...current, visible: false } : null);
      }, Math.max(e.payload.duration_ms - 140, 0));
    });
    return () => {
      if (timer.current) clearTimeout(timer.current);
      unsub.then((f) => f());
    };
  }, []);

  if (!toast) return null;

  const rootStyle: CSSProperties = {
    width: "100vw",
    height: "100vh",
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    pointerEvents: "none",
    overflow: "hidden",
  };
  const pillStyle: CSSProperties = {
    width: "100vw",
    height: "100vh",
    padding: "0 35vh",
    borderRadius: 999,
    display: "flex",
    alignItems: "center",
    justifyContent: "center",
    gap: "25vh",
    background: "rgba(78, 78, 76, 0.96)",
    color: "#f5f5f3",
    boxShadow: "0 18px 50px rgba(0, 0, 0, 0.22)",
    opacity: toast.visible ? 1 : 0,
    transform: toast.visible ? "translateY(0) scale(1)" : "translateY(6px) scale(0.98)",
    transition: "opacity 140ms ease, transform 140ms ease",
  };
  const textStyle: CSSProperties = {
    minWidth: 0,
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
    fontSize: "30vh",
    fontWeight: 650,
    lineHeight: 1,
    letterSpacing: 0,
  };

  return (
    <div style={rootStyle}>
      <div style={pillStyle}>
        <span style={{ width: "31.25vh", height: "31.25vh", display: "flex", flex: "0 0 auto" }}>
          <IconCopy size={1} style={{ width: "100%", height: "100%" }} />
        </span>
        <span style={textStyle}>{toast.message}</span>
      </div>
    </div>
  );
}
