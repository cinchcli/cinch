#!/usr/bin/env node
// Wrapper around `tauri dev` that picks a dev-server port and propagates it
// to both Vite (`vite.config.ts` reads TAURI_DEV_PORT) and Tauri (we inject a
// `--config` overlay so `devUrl` matches).
//
// Port selection:
//   - If TAURI_DEV_PORT is set explicitly, use it as-is (fail loud if busy).
//   - Otherwise start at 1420 and scan upward to the first free port.
//
// Usage:
//   npm run tauri:dev                 # auto-pick from 1420 upward
//   TAURI_DEV_PORT=1430 npm run tauri:dev   # pin a specific port
import { spawn } from "node:child_process";
import net from "node:net";

const SCAN_START = 1420;
const SCAN_END = 1450;

// Probe a single (host, port) pair. Vite resolves "localhost" via the OS,
// which on macOS prefers ::1, so we must check both IPv4 and IPv6 — a
// 127.0.0.1-only probe will miss a listener bound to ::1 and vice versa.
function probe(host, port) {
  return new Promise((resolve) => {
    const srv = net.createServer();
    srv.unref();
    srv.once("error", () => resolve(false));
    srv.listen({ port, host, exclusive: true }, () => {
      srv.close(() => resolve(true));
    });
  });
}

async function isFree(port) {
  for (const host of ["127.0.0.1", "::1"]) {
    if (!(await probe(host, port))) return false;
  }
  return true;
}

async function pickPort() {
  const raw = process.env.TAURI_DEV_PORT;
  if (raw) {
    const port = Number(raw);
    if (!Number.isInteger(port) || port < 1024 || port > 65535) {
      console.error(
        `TAURI_DEV_PORT must be an integer in [1024, 65535] (got: ${raw})`
      );
      process.exit(1);
    }
    return port;
  }
  for (let p = SCAN_START; p <= SCAN_END; p++) {
    if (await isFree(p)) return p;
  }
  console.error(
    `No free port in [${SCAN_START}, ${SCAN_END}]. Set TAURI_DEV_PORT=<n> to override.`
  );
  process.exit(1);
}

const port = await pickPort();
if (port !== SCAN_START) {
  console.log(`[tauri:dev] port ${SCAN_START} busy → using ${port}`);
}

const overlay = JSON.stringify({
  build: { devUrl: `http://localhost:${port}` },
});

const child = spawn(
  "npx",
  ["tauri", "dev", "--config", overlay, ...process.argv.slice(2)],
  {
    stdio: "inherit",
    env: { ...process.env, TAURI_DEV_PORT: String(port) },
  }
);

child.on("exit", (code, signal) => {
  if (signal) process.kill(process.pid, signal);
  else process.exit(code ?? 0);
});
