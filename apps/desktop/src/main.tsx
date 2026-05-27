import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import App from "./App";
import CopyToastOverlay from "./CopyToastOverlay";
import SnapOverlay from "./SnapOverlay";
import { AuthProvider } from "./lib/state/auth";

const isOverlay = new URLSearchParams(window.location.search).has("overlay");
const isCopyToast = new URLSearchParams(window.location.search).has("copy-toast");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isOverlay ? (
      <SnapOverlay />
    ) : isCopyToast ? (
      <CopyToastOverlay />
    ) : (
      <AuthProvider>
        <App />
      </AuthProvider>
    )}
  </React.StrictMode>,
);
