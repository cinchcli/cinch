import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import App from "./App";
import SnapOverlay from "./SnapOverlay";
import { BackgroundHintDialog } from "./components/BackgroundHintDialog";
import { AuthProvider } from "./lib/state/auth";

const isOverlay = new URLSearchParams(window.location.search).has("overlay");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isOverlay ? (
      <SnapOverlay />
    ) : (
      <AuthProvider>
        <App />
        {/* Mounted once here (not inside App's per-branch returns) so the
            one-time background-running hint is shown no matter which screen
            the user dismisses the window from. */}
        <BackgroundHintDialog />
      </AuthProvider>
    )}
  </React.StrictMode>,
);
