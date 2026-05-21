# Cinch

Your clipboard. Across every machine.

This is the monorepo for the Cinch client toolkit:
- `crates/client-core` — shared Rust library (wire schema, crypto, HTTP, WebSocket, storage)
- `crates/cli` — the `cinch` CLI binary
- `apps/desktop` — Cinch Desktop macOS app (Tauri v2)

The relay server lives in a separate repository: [cinchcli/relay](https://github.com/cinchcli/relay).

## Install

**macOS — Desktop + CLI** (recommended):
```bash
brew install --cask cinchcli/tap/cinch
```

**macOS / Linux — CLI only** (headless servers, CI):
```bash
brew install cinchcli/tap/cinch
```

**Linux — curl installer**:
```bash
curl -fsSL https://cinchcli.com/install.sh | sh
```

## Development

Requirements: Rust stable, Node 22+, pnpm, buf, Go (for proto bindings used by the relay).

```bash
# Build everything
make build

# Run tests
make test

# Develop desktop app
make dev-desktop
```

See [docs/](docs/) for architecture and contribution guides.

## License

AGPL-3.0 OR LicenseRef-cinchcli-commercial — see [LICENSE](LICENSE).
