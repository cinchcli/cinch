# CLAUDE.md

This file provides guidance to Claude Code when working with the cinch monorepo.

**Agent worktree convention:**

```bash
./scripts/new-agent-worktree.sh <agent> <task>
# → cinch/<agent>-<task>/ on branch agent/<agent>/<task>
```

The `main/` worktree is the human's reference checkout — agents must not modify files inside it.

## Commands

```bash
make build           # cargo build --workspace + pnpm build (desktop)
make test            # cargo test --workspace + pnpm test + go test ./go/...
make lint            # cargo fmt --check + clippy + buf lint + go vet
make generate        # buf generate + tauri-specta bindings
make dev-desktop     # pnpm tauri dev
make verify-versions # check version parity across all components
```

## Versioning

Single version across all components. Bump in:
- `crates/client-core/Cargo.toml`
- `crates/cli/Cargo.toml`
- `apps/desktop/src-tauri/Cargo.toml`
- `apps/desktop/package.json`
- `apps/desktop/src-tauri/tauri.conf.json`

Or run `./scripts/check-version-parity.sh <expected-version>` to verify.

## Wire schema

`crates/client-core/proto/cinch/v1/*.proto` is the single source of truth. Rust types generated via `prost-build` in `crates/client-core/build.rs`. Go bindings generated via `buf generate` into `go/cinch/v1/` and vendored into the relay repo.

`testdata/wire-vectors.json` is the cross-language compatibility gate. Round-tripped from Rust and Go; relay maintains a byte-equal copy as `relay/internal/wire_test/testdata/wire-vectors.json`.

## Key conventions

- All code, comments, commits, and docs in **English**.
- Never use `any` in TypeScript — define typed interfaces.
- Never use `cinchcli-core` from crates.io — internal path deps only.
- `content_type` wire vocabulary is canonical 4 strings: `text`, `code`, `url`, `image`.
- Generated code is never edited by hand.
