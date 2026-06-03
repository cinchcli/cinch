# Changelog

All notable changes to the `cinch` CLI are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and the project follows [Semantic Versioning](https://semver.org/).

## Unreleased

### Added

- Add `cinch mcp`: a read-only MCP stdio server exposing local clipboard
  history (`search_clipboard`, `list_recent_clipboard`, `get_clipboard_item`)
  to AI tools like Claude Code and Cursor.
- **`cinch send`** — send stdin to your fleet via the relay (E2EE always-on,
  broadcast to all your devices). The explicit fleet-send verb. (Directed send
  to one machine is planned for a later build.)
- **`cinch copy`** / **`cinch paste`** — local clip I/O (pbcopy/pbpaste-shaped).
  `copy` saves stdin to local history (never contacts the relay); `paste`
  prints a local clip to stdout (`latest` | id-prefix | index).
- **Fleet-scoped MCP read** — `search_clipboard` / `list_recent_clipboard`
  accept `scope:"fleet"` to read clips that originated on *other* machines
  (this device excluded), decrypted locally. On a headless reader box set
  `CINCH_MCP_FLEET=1` to enable a lazy, once-per-session backfill on the first
  fleet call. See `docs/headless-send-and-fleet-read.md` for the provisioning
  recipe + the agent loop.

### Changed (breaking)

- **`cinch push` changed meaning and is removed.** Local save is now
  **`cinch copy`**; sending to your fleet is the new **`cinch send`**. Bare
  `cinch push` now **hard-errors** with guidance (exit non-zero) — it will
  never silently save or send. This is a meaning-swap (old `push` = silent
  local save), so it fails loudly rather than doing the opposite of what old
  scripts expect; update scripts/aliases.
- Top-level command surface restructured into hierarchical groups, then
  renamed for the local/fleet two-plane model. `cinch --help` shows the new
  surface. Mapping from the pre-group flat commands:
  - `cinch list` / `search` / `get` / `rm` / `transform` → `cinch history {list, search, show, rm, transform}` (`get`→`show`; `rm` is now variadic + `--local`)
  - `cinch pin <id>` / `unpin <id>` / `pinned` → `cinch pin <ref>` / `cinch unpin <ref>` / `cinch history list --pinned` (both cross-plane by default; `--local` to scope to the local store)
  - `cinch devices` / `pair` / `nickname` / `set-name` / `retention` / `revoke` / `sources` → `cinch fleet {list, add, rename <dev>, rename self, retention, revoke, sources}` (`pair`→`add`; `set-name`+`nickname` merge into `fleet rename`)
  - `cinch plan` / `telemetry` → `cinch account {plan, telemetry}`
  - `cinch auth set-name` → `cinch auth name`
  - `pull`, `ai`, `auth login/status/logout/approve/retry-key/recovery`, `admin`, `completion`, `self-update`, `mcp` are unchanged.
- **Old group spellings still work with a deprecation warning until 0.8.**
  `clip *`→`history *`, `device *`→`fleet *`, `pin add/rm/list`→
  `pin`/`unpin`/`history list --pinned`, `auth set-name`→`auth name` each print
  one stderr note and route to the new handler. Shell completions emit **only**
  the new names from 0.5 — re-run `cinch completion <shell>` to retrain
  muscle memory.
- The dynamic-completion helper now invokes `cinch fleet list --names` (was `cinch device list --names`).
- Desktop now shares its local store with the CLI (single SQLite DB at `~/.cinch/store.db`). On this update, desktop app settings — global shortcut, send shortcut, local/remote retention days, excluded-apps list, per-source auto-copy/alert preferences, and saved window placement — reset to defaults. The "bump recently-copied clip" behavior (`mark_clip_copied`) was removed.

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
