# Desktop App — Developer Rules

## Type Contract

All TypeScript types for commands and events are **auto-generated** from Rust via tauri-specta.

- **Never** define wire types manually in TypeScript.
- **Never** add types to `src/bindings.ts` by hand — this file is overwritten on every `cargo test export_bindings -- --ignored`.
- To add or change a type, edit the Rust source (`src-tauri/src/`) and regenerate.

### Regenerating bindings

```bash
cd src-tauri && cargo test export_bindings -- --ignored
```

This writes `src/bindings.ts` automatically.

## Commands

All Tauri command calls go through the typed `commands` object from `src/bindings`:

```ts
import { commands } from "./bindings";
const clips = await unwrap(commands.listClips(null, null, 100));
```

Never use `invoke<T>(...)` directly — grep for it; zero occurrences is the invariant.

## Events

All Tauri event subscriptions go through the typed `events` object from `src/bindings`:

```ts
import { events } from "./bindings";
const unsub = events.clipReceived.listen((e) => console.log(e.payload));
```

Never use `listen<T>("event-name", cb)` from `@tauri-apps/api/event` — grep for it; zero occurrences is the invariant.

## Cross-repo dependency changes (cinch-core)

Desktop pulls `cinchcli-core` from crates.io. The parent CLAUDE.md states the
"no sibling-checkout invariant": this repo must build standalone, since CI
and fresh clones only check out `desktop/`. Anything that breaks that
invariant turns `main` red on the next push.

When a desktop feature needs an unpublished cinch-core change, the order is:

1. Land the change in `cinchcli/cinch-core`, bump `crates/client-core/Cargo.toml`, and `cargo publish -p cinchcli-core`.
2. **Then** bump `src-tauri/Cargo.toml`'s `cinchcli-core` version in a separate desktop commit.

Do NOT, on the desktop repo:

- Add a `[patch.crates-io]` block with a path that escapes the repo (`../../cinch-core/...`). The path resolves on the maintainer's multi-repo checkout but not in CI or on any other machine, and Cargo check fails with `No such file or directory`.
- Bump a `version =` to a number that is not yet on crates.io. Cargo cannot resolve it and every contributor's build breaks.
- Push a desktop change that compiles only because a local patch override is masking a missing published version. A green local `cargo check` under a `[patch.crates-io]` override is not a signal that CI will pass — verify against the published state (drop the patch block, then `cargo check`) before pushing.

If you need to run desktop locally against an unpublished cinch-core during
development, keep the override out of `Cargo.toml`. Use an uncommitted
`.cargo/config.toml` or a worktree-local `Cargo.toml` patch you never `git add`.

## Content Type Classification

The desktop's clipboard polling pipeline classifies text clips before pushing:

- `clipboard/monitor.rs` calls `client_core::classify::detect(&raw)` on the byte buffer produced by `text.into_bytes()`. The bytes-in API means there's no `&str` / `Vec<u8>` borrow dance and no upfront UTF-8 walk over the clipboard payload.
- `ContentType` derives `Copy`, so the classified value moves cleanly into the spawned async closure.
- The classified value flows into both `pusher.push_text(.., content_type)` (wire) and the `clip_received_stub(.., content_type.as_wire())` event payload (frontend).

Wire vocabulary is exactly 4 strings: `text`, `code`, `url`, `image`. The frontend (`ClipCard.tsx`, `ClipDetail.tsx`, `icons.tsx`) dispatches on these. Do not introduce new values like `json` or `error` on the desktop side — `cinch-core/proto/cinch/v1/clips.proto` is the source of truth, and the wire field is open `string` only for backwards compatibility. Adding a new logical type requires a coordinated cinch-core change + crates.io publish.

`store::models::LocalClip` (the legacy type still derived in `models.rs`) is being phased out. New code should use `commands::clips::LocalClip` (Specta-exported). The legacy type is kept alive only because `sync_status.rs` and a few tests still depend on it.

### Legacy MIME content_type normalization

Pre-2026-05 desktop builds emitted MIME-style strings (`"text/plain"`,
`"image/png"`) on the wire. The relay's `content_type` column is an open
`string`, so those values survived. To keep the React side dispatching on
strict equality (`=== "image"`, `=== "text"`), every `StoredClip`
crossing the Rust→frontend boundary passes through
`commands::clips::normalize_content_type` — which collapses `text/*` to
`"text"` and `image/*` to `"image"`. Apply it at any new boundary that
constructs a `LocalClip` for the frontend (see `stored_to_local`,
`LocalClip::from_legacy`, and `clipboard::monitor::clip_received_stub`
for the three current sites). New producers must continue to emit
canonical strings; this helper is a read-side defense, not a license to
accept MIME on push.

## Release Process

Three manifests carry the desktop version:

- `src-tauri/Cargo.toml` (`version = "..."`) — drives the Tauri build
  output filename (`Cinch_<version>_aarch64.dmg`) and what the macOS app
  bundle reports as its own version.
- `src-tauri/tauri.conf.json` (`"version": "..."`) — Tauri's primary
  source for `CFBundleShortVersionString` and the updater manifest.
- `package.json` (`"version": "..."`) — frontend tooling and pnpm.

**Releasing**: bump all three to the same value, commit, then tag
`desktop-vX.Y.Z` at that commit with the matching version. The release
workflow extracts the version from the tag and expects the assets the
Tauri build produces (named from `Cargo.toml`) to match.

**Why this matters**: desktop-v0.1.{8,9} were silent disasters because
Cargo.toml stayed at `0.1.7` while the tag advanced. The Tauri builder
uploaded `Cinch_0.1.7_aarch64.dmg` under the wrong tag, the
`update-cask` step's `curl -fsSL .../Cinch_0.1.9_aarch64.dmg` 404'd,
the homebrew tap stalled at 0.1.5 for two releases, and anyone who did
manage to download the DMG got an app that reported itself as v0.1.7.

**Enforcement**: `scripts/check-version-parity.sh` (single source of
truth) is wired into three places:

- `lefthook.yml` pre-commit (glob-scoped to the three manifests so it
  stays silent on day-to-day edits)
- `.github/workflows/ci.yml` validate job (parity-only)
- `.github/workflows/desktop-release.yml` build-tauri (parity + tag
  match; aborts before the Tauri build runs)

Run it manually with `bash scripts/check-version-parity.sh` for parity
or `bash scripts/check-version-parity.sh 0.1.10` to also assert the
tag value.

## CLI Embedding (macOS / Windows)

The Cinch desktop binary embeds the `cinch` CLI behind the `builtin-cli`
Cargo feature (on by default; see `src-tauri/Cargo.toml`). At runtime,
`src-tauri/src/main.rs` inspects `argv[0]`'s basename: if it matches
`cinch` (or `cinch.exe` on Windows), the binary dispatches to
`cinch_cli::run()` via `std::process::exit`; otherwise it launches Tauri
via `cinch_desktop_lib::run()`.

### macOS: Cask `target:` rename, no in-bundle symlink

Homebrew Cask exposes the CLI to users with:

```ruby
binary "#{appdir}/Cinch.app/Contents/MacOS/Cinch", target: "cinch"
```

This creates `/opt/homebrew/bin/cinch` → `Cinch.app/Contents/MacOS/Cinch`
at install time. The case difference is load-bearing: double-clicking
the .app invokes `Cinch` (capital, argv[0] → Tauri); running `cinch`
from PATH invokes the same binary with argv[0] == `"cinch"` (CLI
dispatch). Same file, same inode, two routes.

Do NOT add a `cinch` symlink **inside** `Cinch.app/Contents/MacOS/`.
macOS APFS is case-insensitive by default, so `Cinch` and `cinch`
collide inside the bundle — `ln -sf Cinch cinch` deletes the real
binary and leaves a self-referencing symlink. The Cask-managed symlink
at `/opt/homebrew/bin/cinch` lives on a separate filesystem path and
avoids the collision entirely.

This also means the Phase 4 publish pipeline does NOT need any
post-`tauri build` symlink step before code signing / notarization —
the bundle ships with just one binary (`Cinch`), and Homebrew Cask
performs the linking at install time. The Tauri updater replaces the
.app contents in place, and the Cask-managed symlink continues to
resolve correctly afterwards (it points at a path, not an inode).

### Windows: separate `cinch.exe` via `externalBin`

On Windows we don't share one binary. Tauri's `externalBin` config
bundles a separately-built `cinch.exe` next to the desktop installer,
and the MSI puts it on PATH. See
`apps/desktop/scripts/prepare-cli-binary.mjs` (lands in Phase 3 Task 6).

## Files Never to Commit

`.design-research/` and `docs/` (both root-level) hold internal product strategy: personas, journey maps, north-star vision, dashboard specs. They are gitignored. Do not move them out of ignore status; if they need to live in version control, put them in a private repo instead.
