# AI-Era Clipboard — `cinch mcp` (clipboard history as an MCP server)

- **Date:** 2026-05-24
- **Status:** Design — revised after codex review (2026-05-24)
- **Author:** brainstormed with the maintainer
- **Scope:** v1 wedge of the "AI-era clipboard" direction for cinch

## 1. Problem & context

cinch is already a cross-device clipboard: synced history (relay + WebSocket
+ E2EE), a local SQLite store with FTS5 search, content-type classification
(`text` / `code` / `url` / `image`), and a desktop menu-bar agent with a
global-hotkey recall window.

Two real pains motivated this work (from the maintainer's own daily use):

1. **Re-finding old copies** — "where did that thing I copied go?"
2. **AI ping-pong** — manually shuttling context (errors, code, answers)
   between browser AI chat, terminal CLI agents (Claude Code, Codex), the
   IDE (Cursor), and between different models. The ping-pong happens in
   **all** of these environments.

Root cause: everything you copy is *working context*, but the clipboard
forgets it and forces **you** to be the courier between every AI tool.
AI coding tools pull context via file/URL/terminal references
(`@file`, `@terminal`, `@`-symbols) — transient clipboard content has **no
first-class path in**, so you re-paste it every time.

## 2. Guiding principle — human first

The clipboard must be excellent for the **human** to use every day *first*;
AI/agent access is a compounding multiplier, never the lead. This principle
holds here precisely because the human recall experience **already exists**
(see §4) — so the AI layer builds on a satisfied human foundation rather
than substituting for it.

## 3. North-star & positioning

**North-star:** a clipboard-first, synced, model-neutral **working memory** —
everything you copy is a searchable memory that any AI (browser, terminal,
IDE) can draw from.

**Positioning (validated by landscape research):**

- Not "another clipboard manager" — that space is saturated and has no AI
  (only Raycast has real clipboard AI, and it deliberately won't sync
  history or expose an agent API).
- Not "capture-everything memory" — Pieces for Developers already owns that
  (OS-level OCR capture + 9-month recall + a shipped MCP server), is heavy,
  single-machine, and not clipboard-first; a head-on fight is unwise.
- **The opening:** "the clipboard every AI can query" — lean, synced,
  private, clipboard-first. cinch's existing **sync + content
  classification + secret-skipping/E2EE** is exactly the combination no
  existing clipboard-MCP prototype has (the handful on GitHub are
  single-machine, sub-10-star, current-clipboard-only).

## 4. What already exists (do not rebuild)

A prior code audit confirmed the human recall path is ~80% built:

- **Unified store** `~/.cinch/store.db`, shared by **both** CLI and desktop.
  `clips` table stores **decrypted plaintext** content (the relay only ever
  sees ciphertext; decryption happens client-side in the WS layer). FTS5
  virtual table `clips_fts` indexes `content`.
- `client_core::store::queries::{search_clips, list_clips}` — reusable by
  both CLI and desktop; the CLI already uses them. Note the actual
  `list_clips` signature filters by `source`, `since_ms`, `pinned_only`,
  and `limit` — there is **no `content_type` filter** today.
- Desktop: global hotkey (default `Cmd+Shift+V`) → summons a borderless,
  transparent window; captures the frontmost app PID *before* stealing
  focus; restores it afterward (`focus_previous_app`). NSPasteboard
  read+write. Search UI (SearchBar + ClipList + ClipCard + ClipDetail).
- The **CLI already writes to the clipboard** (correcting the earlier
  assumption): `cinch pull --copy` uses `arboard::Clipboard::set_text`
  (`crates/cli/src/commands/pull.rs:506`); image write is macOS-only via a
  CLI NSPasteboard helper (`pull.rs:525`). This matters for the deferred
  write tool (§7): centralize, don't rebuild.

**Decision:** the human recall experience is good enough for v1. We do **not**
add auto-paste (auto-⌘V) — it is unpredictable, can paste into the wrong
target, breaks per-app, and removes user control. The existing flow
(select → copy → focus returns → user presses ⌘V) stays. "Smart ranking"
(recency/frequency/pinned/current-app) is noted as future polish, not v1.

## 5. v1 scope — `cinch mcp` (read-only)

A stdio MCP server shipped as a new `cinch mcp` subcommand. AI tools
(Claude Code, Cursor, Cline) spawn it; it reads `~/.cinch/store.db` and
exposes the synced clipboard history. No new store, no encryption key
(plaintext is local), no daemon; works whether or not the desktop app is
running.

**v1 is read-only.** The guarded write tool (`set_clipboard`) is deferred to
v2 (§7) per codex review: ship the read loop first, add write once the server
is stable and the visibility/notification story exists.

### 5.1 MCP tool surface

| Tool | Behavior |
|---|---|
| `search_clipboard(query, limit?)` | FTS5 search over history (input-sanitized, §5.3). "find that error I copied last week" |
| `list_recent_clipboard(limit?, source?)` | Recent clips, optional `source` filter. `limit=1` ≈ "what I just copied". (`content_type` filter omitted in v1 — `list_clips` does not support it yet; adding it is a query change, see §9) |
| `get_clipboard_item(id)` | Full content of one clip (lists return previews only) |

### 5.2 Response shape & token discipline

Per clip: `id`, `content` (text only — see image rule below), `content_type`
(normalized to `text`/`code`/`url`/`image`), `source`, `created_at`
(ISO 8601), `byte_size`.

- `search`/`list` results **truncate** `content` to a preview to avoid
  blowing the AI context window; full content via `get_clipboard_item`.
- **Image rule (hard):** never serialize `StoredClip.content` (a `BLOB`)
  generically. Image clips return metadata only (`content_type: "image"`,
  `byte_size`, `media_path`), never raw bytes — including from
  `get_clipboard_item`.
- **content_type normalization:** the store may hold legacy MIME-style
  values (`text/*`, `image/*`) alongside the canonical four. Normalize to
  the canonical set at the MCP boundary (reuse the existing
  `normalize_content_type` helper) so AI consumers see a stable vocabulary.
- **Empty results:** return a structured empty array (plus optional
  diagnostic field), never a human-readable stdout/stderr message — stdio
  is the MCP transport, not a console.

### 5.3 Robustness (from codex review)

- **FTS input sanitization:** `search_clips` passes raw input into
  `clips_fts MATCH ?1`. Natural-language AI queries with punctuation,
  quotes, `-`, `:` can raise FTS5 syntax errors. The MCP layer must
  sanitize/quote/tokenize the query (or fall back to a `LIKE` scan) so a
  query never errors out the tool call.
- **Limit clamping:** clamp `limit` to a sane default and a hard maximum;
  reject negative/zero. Never pass arbitrary client `i64` straight through.

### 5.4 Quiet runtime (critical for stdio)

- Do **not** reuse `runtime::open_ctx()` — it requires an auth token and
  builds a `RestClient`, which contradicts "local store only, never touches
  relay or key" (`crates/cli/src/runtime.rs:29`). Open
  `Store::open(default_db_path())` directly.
- Suppress the normal CLI wrapper side effects on this path: no update
  notices, no telemetry, no opportunistic backfill/session flush, no stderr
  chatter (`crates/cli/src/lib.rs:225`). Any stray stdout/stderr corrupts
  the MCP stdio stream.

### 5.5 Privacy & guards

- Reads only the local decrypted store; never touches the relay or the key.
- **Honest framing:** an MCP read surface gives any configured AI client
  **bulk access to all plaintext local clipboard history**. "Read-only"
  prevents writes, not exfiltration. The desktop's "skips password managers"
  behavior is a macOS-specific NSPasteboard concealed-type guard at *capture*
  time — it is not a universal guarantee that secrets are absent from the
  store.
- Mitigation for v1: an **exposure-scope** option (e.g., "last N days only",
  exclude specific sources/content types) so the surface is bounded by
  default rather than exposing the full history.

### 5.6 Code location & dependencies

- New subcommand: `crates/cli/src/commands/mcp.rs`; register it in the `Cmd`
  enum and `crates/cli/src/commands/mod.rs`.
- Wraps existing `client_core::store::queries::{search_clips, list_clips}`.
- MCP protocol: evaluate the Rust SDK `rmcp` vs a minimal hand-rolled
  JSON-RPC-over-stdio (decide in planning).

### 5.7 Integration (dogfooding path)

- Claude Code: `claude mcp add cinch -- cinch mcp` (or `.mcp.json`).
- Cursor: register `cinch mcp` in `mcp.json`.

### 5.8 Sequencing & tests

- Add `mcp` to the `Cmd` enum + `commands/mod.rs`; wire the quiet,
  store-only runtime path; implement the three read tools.
- Tests: MCP protocol round-trip, malformed/punctuated FTS queries (must not
  error), limit clamping (negative/zero/over-max), image clips never return
  bytes, content_type normalization of legacy MIME values, empty-store path.

## 6. Edge cases

- `store.db` missing / empty → structured empty results.
- Offline clips (`synced = 0`) are included (correct — they are real local
  clips).
- Concurrency: SQLite **WAL + `busy_timeout`** make concurrent reads safe
  alongside desktop/CLI writers (`crates/client-core/src/store/mod.rs:41`).
  `~/.cinch/sync.lock` coordinates the sync/backfill writers specifically,
  not every local mutation — do not over-claim it as the general guard.

## 7. Out of scope (future)

- **Guarded write (`set_clipboard`)** — v2. The write path already exists
  (`arboard` text in `pull.rs:506`; macOS NSPasteboard image helper in
  `pull.rs:525`), so v2 is centralization + guards, not new plumbing:
  opt-in `--allow-write`, **text only** first, size cap, local clipboard
  only (no cross-device push), and a desktop notification so the human
  always knows the AI wrote.
- Semantic/embedding search (FTS5 keyword search is the v1 recall).
- Transform-on-recall (KO↔EN / summarize / to-table) — strong v2 candidate;
  hits the maintainer's personal bilingual pain and is the most "AI-era"
  hook, but adds AI calls and UI surface.
- Cross-device push / device targeting via MCP write.
- Smart ranking of recall (recency × frequency × pinned × current app).
- `content_type` filter on `list_recent_clipboard` (needs a `list_clips`
  query change).
- Desktop UI for MCP onboarding.

## 8. Success criteria

1. With `cinch` added to Claude Code, *"fix the error I just copied"* and
   *"pull the API spec I copied last week"* work **without manual
   copy/paste**.
2. A natural-language query with punctuation never errors the tool call.
3. In the maintainer's own dogfooding, the number of manual copy/pastes
   done purely to carry context to an AI drops noticeably.

## 9. Open questions for planning

- `rmcp` SDK vs hand-rolled JSON-RPC/stdio.
- FTS sanitization approach (quote/tokenize vs `LIKE` fallback).
- Preview truncation length and `byte_size` thresholds; limit default + max.
- Exposure-scope default (full history vs last-N-days out of the box).
- Where to centralize the CLI clipboard-write path for the v2 write tool.
