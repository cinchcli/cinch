# Cinch

Remote clipboard for developer context. Local history. Cross-machine clipboard. MCP tools.

This is the monorepo for the Cinch client toolkit:
- `crates/client-core` — shared Rust library (wire schema, crypto, HTTP, WebSocket, storage)
- `crates/cli` — the `cinch` CLI binary
- `apps/desktop` — Cinch Desktop macOS app (Tauri v2)

The relay server lives in a separate repository: [cinchcli/relay](https://github.com/cinchcli/relay).

## What it does

- **Local-first clipboard history** — terminal output, copied text, and desktop clipboard entries live in `~/.cinch/store.db` on your machine.
- **Remote clipboard / self-hostable relay** — move clips between terminal and desktop through a hosted or self-hosted relay; the relay stores ciphertext only.
- **MCP + transforms for AI workflows** — expose local clipboard history to AI tools, transform/redact text, or explicitly prepare terminal errors with `cinch ai fix`.

## Install

**macOS — Desktop + CLI** (recommended):
```bash
brew install --cask cinchcli/tap/cinch
```

**macOS — CLI only** (headless servers, CI):
```bash
brew install cinchcli/tap/cinch
```

**Linux — curl installer**:
```bash
curl -fsSL https://cinchcli.com/install.sh | sh
```

> **Short alias:** `ci` is installed as a shorthand for `cinch` — every command works under both names (`ci pull`, `ci send`, …).

### AI workflow v1

`cinch ai fix` turns terminal, log, or error output into an AI-ready debugging prompt. It only calls a provider when you explicitly configure or select one; `--no-send` never calls an AI provider.

```bash
cargo test 2>&1 | cinch ai fix --no-send
cat error.log | cinch ai fix
cinch ai fix latest --no-send
cinch ai fix 01HXABCD --no-send
```

Provider boundary:

- `hosted-bedrock` — Cinch-operated managed provider for hosted distributions.
- `bedrock-byok` — BYOK AWS Bedrock boundary for self-hosters/developers.
- `openai-compatible` — Ollama, LM Studio, llama.cpp, or OpenAI-compatible endpoints.

### Private clipboard for your AI tools (MCP)

`cinch mcp` exposes your local clipboard history (read-only) to MCP-aware AI
tools so they can search and fetch what you copied — no manual paste. It runs
against `~/.cinch/store.db` on your machine and does not contact the relay.

Claude Code:
```bash
claude mcp add cinch -- cinch mcp
```

Cursor (`~/.cursor/mcp.json` or project `.cursor/mcp.json`):
```json
{ "mcpServers": { "cinch": { "command": "cinch", "args": ["mcp"] } } }
```

Tools: `search_clipboard`, `list_recent_clipboard`, `get_clipboard_item`.
Read-only; local-only; no relay/network access.

Privacy: this exposes local clipboard history to the AI client you configure.
Hosted relay retention (7 days by default) does not limit local history.
To narrow what MCP returns, set `CINCH_MCP_MAX_AGE_DAYS` (e.g. `90`); unset
(default) exposes full local history.

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
