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

### Clipboard for your AI tools (MCP)

`cinch mcp` exposes your synced clipboard history (read-only) to MCP-aware AI
tools so they can search and fetch what you copied — no manual paste.

Claude Code:
```bash
claude mcp add cinch -- cinch mcp
```

Cursor (`~/.cursor/mcp.json` or project `.cursor/mcp.json`):
```json
{ "mcpServers": { "cinch": { "command": "cinch", "args": ["mcp"] } } }
```

Tools: `search_clipboard`, `list_recent_clipboard`, `get_clipboard_item`.
Read-only; runs locally against `~/.cinch/store.db`; no relay/network access.

Privacy: this exposes your local clipboard history to the AI client. To bound
the surface, set `CINCH_MCP_MAX_AGE_DAYS` (e.g. `90`) so only recent clips are
returned; unset (default) exposes full history.

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
