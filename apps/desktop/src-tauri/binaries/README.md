# Tauri externalBin staging

Built CLI binaries are staged here for Tauri's `externalBin` bundling
mechanism. The Phase 3 Task 6 script
(`apps/desktop/scripts/prepare-cli-binary.mjs`) builds the standalone
`cinch` CLI for the current target and copies the result to
`cinch-<target-triple>(.exe)` in this directory.

This directory itself is tracked in git (to keep the staging path
valid); the binaries are `.gitignore`d.

This staging is only used on Windows builds (`externalBin` does not
ship CLI on macOS — see the project's `apps/desktop/CLAUDE.md`
"CLI Embedding" section for the macOS approach).

The `externalBin` entry lives in `tauri.windows.conf.json` (sibling of
`tauri.conf.json`), not in the shared config. Tauri auto-merges
`tauri.<platform>.conf.json` overlays only when building for that
platform, so macOS `cargo build` / `tauri build` don't try to resolve
`binaries/cinch-aarch64-apple-darwin` and fail on a fresh checkout.
