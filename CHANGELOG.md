# Changelog

All notable changes to the `cinch` CLI are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and the project follows [Semantic Versioning](https://semver.org/).

## 0.1.9 — 2026-05-18

### Added

- Per-device version reporting in `cinch devices` — CLI rows now display the client version (`cinch/0.1.9`) and an `(outdated)` marker for stale peers.
- Stderr nudge when the running CLI itself is outdated relative to the relay's latest known version, so users notice without breaking scripting on stdout.

### Changed

- Generalized Mac-specific copy in `cinch auth login` and `cinch pull` messaging — paired devices may run on Linux or Windows, not just macOS.

### Internal

- Bumps `cinchcli-core` to 0.1.8 and wires `ClientInfo` through every `RestClient` / `WsConfig` call site so the relay can record per-device version on each request.
- Adds `semver` and `is-terminal` deps to support the version comparison and TTY-aware nudge.

## 0.1.8 — 2026-05-17

### Added

- `cinch admin invite {create,list,revoke}` and `cinch admin user {list,remove}` — self-host operators can now manage invite codes and user accounts directly from the CLI. Thin HTTP clients over the relay's `/admin/*` endpoints with bearer auth; exit codes follow the existing conventions (`AUTH_FAILURE` for 401/403, `GENERIC_ERROR` otherwise).

### Internal

- Bumps `cinchcli-core` to 0.1.5, picking up the `invite_code` and `display_name` fields on `LoginRequest`.
- `smoke.yml` updated for the invite-token-gated `/auth/login` flow.

## 0.1.7 — 2026-05-16

### Added

- `cinch auth recovery show` / `restore <phrase>` / `verify <phrase>` — backup the per-user AES-256 encryption key as a 24-word BIP39 phrase, restore it on a new device, or verify a recorded phrase without writing anything. `show` prints to stdout (so the phrase can be piped into a password manager) but refuses to write to a non-TTY without `--yes`; `restore` asks before overwriting a different key already on disk.

### Internal

- Bumps `cinchcli-core` to 0.1.4 for the new `client_core::recovery` module.
- Release workflow GitHub Actions migrated to their Node 24 runtime majors (`upload-artifact` v7, `download-artifact` v8, `gh-release` v3, `cache` v5).

## 0.1.6 — 2026-05-15

### Internal

- Consume `cinchcli-core` from crates.io (cinch-core extraction). The shared client library — crypto, credstore, http, ws, store, sync, generated proto types — is no longer vendored; this repo now ships only the `cinch-cli` binary.
- Bumps `cinchcli-core` to 0.1.2, picking up the SSH-pair 30-second timeout fix and the consolidated `key_exchange::handle_event` handler.

## 0.1.5 — 2026-05-14

### Added

- `cinch search <query>` — FTS5 full-text search across the local clip store.
- `cinch get <id-prefix>` — print a single clip's content (or metadata with `--meta`).
- `cinch pin <id-prefix>` / `cinch unpin <id-prefix>` / `cinch pinned` — manage pinned clips from the terminal.
- `cinch rm <id-prefix>` — delete a clip from the relay and the local store (with TTY confirmation; `--force` to skip).
- `cinch sources` — list distinct push-source machines.
- `cinch nickname <device-id-prefix> <name>` / `--clear` — rename a paired device.
- `cinch revoke <device-id-prefix>` — revoke a paired device's token (self-revoke requires uppercase `YES`).
- `cinch retention [--device <id|self>] [--days N]` — view or set per-device clip retention.
- `cinch list --pinned` — filter listings to pinned clips only.

### Changed

- `cinch devices` now defaults to a **merged view** (paired devices + source-only machines that have pushed), matching the desktop's Machines panel. Pass `--paired-only` for the previous behavior. Scripts that parsed the old output should add `--paired-only`.
- `cinch list` now reads from the local store at `~/.cinch/store.db`. Pass `--remote` to bypass the cache and hit the relay directly.
- All clipboard data (clips, pinned, sources, device cache, retention/alert prefs) now lives in a single SQLite database at `~/.cinch/store.db`. The desktop app migrates its legacy database into this location on first launch; the old DB is renamed to `<old>.db.bak` and is **not** deleted.

### Internal

- New `client-core::store` and `client-core::sync` modules; the desktop app's clip/store/WebSocket code now delegates to them via a path dependency.
- Coordinated writer ownership via an advisory lockfile at `~/.cinch/sync.lock`: the desktop holds the WebSocket while running; the CLI does opportunistic REST backfills when the desktop is absent.
- No relay changes.
