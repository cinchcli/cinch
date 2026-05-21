# Cinch Desktop

Your clipboard. Across every machine.

Cinch Desktop is the macOS companion for [Cinch](https://cinchcli.com): every `cinch push` from a remote SSH box, Docker container, or CI job lands in your system clipboard before you reach Cmd+V. Built with Tauri v2 (Rust backend + React frontend), it delivers real-time WebSocket sync, end-to-end encryption, and a searchable local clip history — all from the menu bar.

## Features

- **Real-time sync** — clips arrive instantly via WebSocket; no polling
- **End-to-end encryption** — AES-256-GCM with X25519 key exchange; the relay only sees ciphertext
- **Full-text search** — SQLite FTS5 index over your entire clip history
- **Multi-relay profiles** — connect to multiple relay servers and switch between them
- **Pin & organize** — pin important clips for quick access
- **Device management** — view paired devices, check versions, revoke access
- **SSH pairing** — add remote machines via `cinch://` deep links or manual token
- **Privacy-aware clipboard** — skips password manager apps (1Password, Bitwarden, LastPass, Keychain Access) and macOS concealed/transient pasteboard types
- **Menu bar tray** — lives in the macOS menu bar; configurable global shortcut
- **Auto-update** — signed updates via GitHub Releases (minisign verification)
- **Retention sweep** — clips older than the configured retention period are pruned automatically
- **Launch at login** — optional autostart via Tauri plugin

## Requirements

- macOS 10.15+ (Apple Silicon or Intel)
- A Cinch relay server — use the hosted relay at `api.cinchcli.com` or [self-host your own](https://github.com/cinchcli/relay)

For development:

- Node.js 22+ and pnpm
- Rust stable toolchain (via `rustup`)

## Development

```bash
pnpm install
make dev        # or: pnpm tauri dev
```

### Build

```bash
make build      # .app + .dmg via Tauri
```

### Test

```bash
make test       # vitest (TypeScript)
make check      # cargo check (Rust)
make clippy     # cargo clippy -D warnings
```

### Regenerate Rust → TypeScript bindings

All TypeScript types are auto-generated from Rust via tauri-specta. Never edit `src/bindings.ts` by hand.

```bash
cd src-tauri && cargo test export_bindings -- --ignored
```

## Architecture

```
desktop/
├── src/                  # React frontend
│   ├── components/       #   UI components (clip list, devices, settings, dialogs)
│   ├── lib/              #   Shared utilities (auth state, fuzzy search, filters)
│   ├── bindings.ts       #   Auto-generated Tauri command + event types
│   └── App.tsx           #   Main app shell
├── src-tauri/
│   └── src/
│       ├── commands/     #   Tauri commands (clips, auth, relays, updater)
│       ├── auth/         #   Auth state machine + credential management
│       ├── clipboard/    #   macOS pasteboard polling + content classification
│       ├── store/        #   SQLite local clip store
│       ├── ws.rs         #   WebSocket client (auto-reconnect with backoff)
│       ├── tray.rs       #   Menu bar icon + tray menu
│       ├── events.rs     #   Tauri event definitions
│       └── lib.rs        #   App initialization + Specta builder
└── Makefile
```

The Rust backend depends on `cinchcli-core` (published on crates.io) for crypto, wire types, sync, and the unified store at `~/.cinch/store.db` — shared with the CLI.

## Data storage

Clips are stored locally in `~/.cinch/store.db` (SQLite + FTS5). On first launch after upgrade, the legacy database at `~/Library/Application Support/com.cinchcli.desktop/cinch.db` is migrated automatically; the old file is renamed to `*.db.bak`.

## Telemetry

Official cinch desktop builds (DMG from GitHub Releases) send anonymous usage stats to help prioritize features and catch breakage. No PII, no clipboard contents, no IP addresses. Source-built binaries (`cargo build` without the release secret) have telemetry compiled out and send nothing.

What's collected: app open events, pairing outcomes, OS/arch, app version. That's it.

Opt out: set `TELEMETRY_DISABLED=1` or `DO_NOT_TRACK=1` in your environment, or `touch ~/.cinch/telemetry_opt_out`. Details: [cinchcli.com/telemetry](https://cinchcli.com/telemetry).

## Links

- Website: [cinchcli.com](https://cinchcli.com)
- CLI: [github.com/cinchcli/cinch](https://github.com/cinchcli/cinch)
- Relay: [github.com/cinchcli/relay](https://github.com/cinchcli/relay)
- Docs: [cinchcli.com/docs](https://cinchcli.com/docs)

## License

Cinch Desktop is proprietary software. See [LICENSE](LICENSE).

The relay server and CLI are open source under AGPL-3.0:
[github.com/cinchcli/relay](https://github.com/cinchcli/relay) · [github.com/cinchcli/cinch](https://github.com/cinchcli/cinch)

For licensing inquiries: [jingmuio@gmail.com](mailto:jingmuio@gmail.com)
