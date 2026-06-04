# `cinch send` — Design Spec

Date: 2026-06-04
Status: Approved (brainstorm) → implementation
Scope: cinch monorepo (`crates/cli` only; reuses existing `crates/client-core`).
No relay / proto / desktop change.
Test platform: macOS (first cut).

## Problem

The CLI is asymmetric. `cinch pull` is a headless cross-machine **read** (it
hits the relay `GET /clips/latest`), but there is no headless cross-machine
**write**. `cinch push` is local-only — the relay is never contacted
(`crates/cli/src/commands/push.rs`: *"this command is local-only; the relay is
never contacted"*). The desktop has a "Send" action (broadcast a clip to all the
user's devices via the relay), but no CLI equivalent exists.

The desktop-settings-replan spec (`specs/2026-06-04-desktop-settings-replan-design.md`)
explicitly flags the gap: *"there is no `cinch send` verb."* This spec fills it.

## Goal

`cinch send` — the headless write counterpart to `cinch pull`. Read stdin,
encrypt + push to the relay (which broadcasts to all the user's devices over
WebSocket), record the clip in this machine's local history, and place it on
this machine's system clipboard.

It is a thin command over the existing `client_core::sync::LocalPusher`
pipeline — the same code path the desktop "Send" uses — so there is **zero
duplication** of the encrypt / push / local-write-through logic.

| verb | behavior |
|------|----------|
| `cinch push` | local store only (relay never contacted) |
| **`cinch send`** | **encrypt + relay broadcast + local store + local clipboard** |
| `cinch pull` | relay read → stdout |

## Command surface

```
cinch send [-l LABEL] [--type TYPE] [--text] [-s|--silent] [--no-copy] [--token T] [--relay URL]
```

- reads stdin (≤ 20 MB), classified exactly like `cinch push`: text / code / url
  auto-detected; images by magic bytes; video rejected.
- `-l, --label` — clip label.
- `--type` — force content type (`image` | `image/*`); mirrors push.
- `--text` — force text mode (skip binary detection).
- `-s, --silent` — suppress the success line (errors still print).
- `--no-copy` — skip the system-clipboard set (relay + local store still happen).
- `--token` / `--relay` — auth overrides (same as pull); also honor
  `CINCH_TOKEN` / `CINCH_RELAY_URL`.

## Data flow

1. `auth_state::ensure_authenticated()` — send REQUIRES a token + relay (unlike
   push's stateless local path). On failure: `Run: cinch auth login`.
2. Read stdin → shared helper `commands::shared::read_and_classify_stdin`
   (extracted from push): enforces the 20 MB cap, runs image/video magic-byte
   detection, applies `client_core::classify::detect` for the text subtype,
   rejects video, honors `--text` / `--type`. Returns `(Vec<u8>, ContentType)`.
3. Load encryption key: `credstore::read_encryption_key(&cfg.user_id) ->
   Option<[u8; 32]>` (same as pull).
4. Build the relay client (`RestClient` from cfg, as pull does) and
   `LocalPusher::new(Arc<Store>, Arc<dyn ClipTransport>, enc_key)`.
   `source = remote:<hostname>` via `machine::hostname_or_unknown()`.
5. Push:
   - text → `LocalPusher::push_text(raw, &source, label, content_type)`
   - image → `LocalPusher::push_image_png(raw, &source, label)`

   Outcomes:
   - key present + relay reachable → encrypt + `POST /clips` → relay broadcasts →
     local write-through with the relay-assigned id → `PushOutcome::Synced(id)`.
   - no key / transient relay error → `PushOutcome::Queued(local_id)` (saved
     locally `Pending`, retried later by a running cinch app).
6. System clipboard (design decision: relay + local + clipboard):
   - text → `io::copy_text_to_clipboard(&text)` (best-effort; a clipboard
     failure never fails the command).
   - image → skipped in cut 1 (no image-clipboard helper in the CLI yet); a
     one-line note is printed. push + local store still happen.
   - `--no-copy` → skip entirely.
7. Output (soft-success; exit 0 in both outcomes):
   - Synced: `✓ Sent {size} to your devices · copied · {ms} ms (id={clip_id})`
   - Queued: `✓ Copied + saved locally · queued for your devices (offline) —
     syncs when a cinch app reconnects.`

## Files

- **new** `crates/cli/src/commands/send.rs` — the command (clap `Args`, `run`).
- `crates/cli/src/commands/mod.rs` + `crates/cli/src/lib.rs` — register
  `Cmd::Send(commands::send::Args)` (help: "Send stdin to all your devices."),
  add to the `command_name` match and the dispatch match.
- `crates/cli/src/commands/shared.rs` — add `read_and_classify_stdin` (+ the
  detection helpers moved from push); refactor `push.rs` to call it so the
  stdin-read + classification logic has one home.
- **reuses** (no change): `client_core::sync::LocalPusher`,
  `client_core::credstore::read_encryption_key`,
  `crate::io::copy_text_to_clipboard`, `crate::auth_state::ensure_authenticated`,
  `client_core::machine::hostname_or_unknown`, `RestClient`.
- No proto / relay / desktop / wire changes.

## Testing

- **Unit (shared helper):** `read_and_classify_stdin` over text / code / url /
  image (magic bytes) / video (rejected) / size-cap / `--text` / `--type` —
  moved and extended from push's existing `detect_content_type` tests so
  coverage is preserved.
- **Unit (send):** Synced vs Queued message selection driven by a mock
  `ClipTransport` (as `backlog_flusher` tests do); `--no-copy` skips the
  clipboard call; the image path skips the clipboard set; `ensure_authenticated`
  guard error when unauthenticated.
- **Manual (macOS, authed):** `echo "hi from $(hostname)" | cinch send` → a
  second paired device receives the clip; `cinch list` shows it locally; the
  system clipboard holds "hi…". Relay-down → soft-success "queued" message, clip
  stored `Pending` locally.
- **Gates:** `cargo build -p cinch-cli`, `cargo test -p cinch-cli`,
  `cargo clippy`, `cargo fmt --check`.

## Out of scope (cut 1)

- Image → system clipboard set (text-only clipboard; images still push + store).
- `--to <device>` targeting — the data model is broadcast-only (no per-device
  targeting); not built.
- Desktop / UI changes; Phase-2 niceties.
