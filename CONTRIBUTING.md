# Contributing to Cinch

Cinch is an open-source remote clipboard for developer context: local clipboard history, cross-machine clipboard through a hosted or self-hosted relay, and MCP tools.

## Development Setup

Requirements: Rust stable, Node 22+, pnpm, buf, and Go.

```bash
make build
make test
make lint
```

Keep diffs small and focused. Prefer existing modules and patterns over new abstractions. Do not add dependencies unless the issue requires it and the tradeoff is clear.

## Good First Issues

- Add a new `cinch clip transform` action for a common developer workflow.
- Add tests for `cinch ai fix --no-send` prompt assembly edge cases.
- Add an MCP tool example for "find the latest error log copied today".
- Improve `cinch ai` provider setup docs for Ollama, LM Studio, or llama.cpp.
- Add a Docker Compose example with Cloudflare Tunnel sidecar wiring.
- Add a Fly.io self-hosting smoke test checklist.
- Add redaction fixtures for tokens, API keys, and connection strings.
- Improve desktop empty states around local history and AI workflow onboarding.
- Add docs showing hosted-to-self-hosted relay migration.
- Expand CLI examples for CI, SSH, tmux, and container workflows.

## AI Workflow Contributions

`cinch ai` must stay explicit. Clipboard or local-store content must never be sent to an AI provider during `push`, `pull`, desktop sync, or `cinch mcp`.

Provider work should keep a clear boundary:

- `hosted-bedrock` is for Cinch-operated managed provider endpoints.
- `bedrock-byok` is for user-owned AWS Bedrock credentials.
- `openai-compatible` is for local or compatible `/v1/chat/completions` endpoints.

Every provider change needs a `--no-send` regression test or an equivalent proof that prompt assembly does not perform network calls.

## Self-Host Examples

The first self-host examples to improve are:

- Docker Compose + Cloudflare Tunnel
- Fly.io + managed Postgres
- Docker Compose + S3-compatible media backend

Keep examples copy-pasteable and include the client login command:

```bash
cinch auth login --relay https://relay.example.com
```
