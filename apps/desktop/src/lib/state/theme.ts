import { useEffect, useState } from "react";

// Three modes: 'light', 'dark', 'system'. 'system' tracks the OS via
// prefers-color-scheme — on macOS Auto, that already flips at the real
// sunrise/sunset boundary tied to Location Services.

export type Theme = "dark" | "light";
export type ThemeMode = Theme | "system";

const THEME_STORAGE_KEY = "cinch-theme";

function systemPreference(): Theme {
  return window.matchMedia("(prefers-color-scheme: light)").matches
    ? "light"
    : "dark";
}

function resolveMode(): ThemeMode {
  const saved = localStorage.getItem(THEME_STORAGE_KEY);
  if (saved === "light" || saved === "dark" || saved === "system") return saved;
  return "system";
}

export function useTheme(): {
  mode: ThemeMode;
  theme: Theme;
  setMode: (m: ThemeMode) => void;
} {
  const [mode, setModeState] = useState<ThemeMode>(resolveMode);
  const [systemTheme, setSystemTheme] = useState<Theme>(systemPreference);

  // Track OS preference so 'system' mode reflows the moment macOS flips.
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: light)");
    const handler = (e: MediaQueryListEvent) => {
      setSystemTheme(e.matches ? "light" : "dark");
    };
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, []);

  const theme: Theme = mode === "system" ? systemTheme : mode;

  useEffect(() => {
    document.documentElement.classList.toggle("light", theme === "light");
  }, [theme]);

  const setMode = (next: ThemeMode) => {
    localStorage.setItem(THEME_STORAGE_KEY, next);
    setModeState(next);
  };

  return { mode, theme, setMode };
}
