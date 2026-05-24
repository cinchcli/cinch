# AI-Era Clipboard — `cinch mcp` (clipboard history as an MCP server)

- **Date:** 2026-05-24
- **Status:** Design (approved in brainstorming; pending spec review)
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
  both CLI and desktop; the CLI already uses them.
- Desktop: global hotkey (default `Cmd+Shift+V`) → summons a borderless,
  transparent window; captures the frontmost app PID *before* stealing
  focus; restores it afterward (`focus_previous_app`). NSPasteboard
  read+write. Search UI (SearchBar + ClipList + ClipCard + ClipDetail).

**Decision:** the human recall experience is good enough for v1. We do **not**
add auto-paste (auto-⌘V) — it is unpredictable, can paste into the wrong
target, breaks per-app, and removes user control. The existing flow
(select → copy → focus returns → user presses ⌘V) stays. "Smart ranking"
(recency/frequency/pinned/current-app) is noted as future polish, not v1.

## 5. v1 scope — `cinch mcp`

A stdio MCP server shipped as a new `cinch mcp` subcommand. AI tools
(Claude Code, Cursor, Cline) spawn it; it reads `~/.cinch/store.db` and
exposes the synced clipboard history. No new store, no encryption key
(plaintext is local), no daemon; works whether or not the desktop app is
running.

### 5.1 MCP tool surface

Read tools (always available):

| Tool | Behavior |
|---|---|
| `search_clipboard(query, limit?)` | FTS5 search over history. "find that error I copied last week" |
| `list_recent_clipboard(limit?, content_type?, source?)` | Recent clips, optional filters. `limit=1` ≈ "what I just copied" |
| `get_clipboard_item(id)` | Full content of one clip (lists return previews only) |

Guarded write tool (opt-in only — see §5.3):

| Tool | Behavior |
|---|---|
| `set_clipboard(content, content_type?)` | Places content on the **local** system clipboard so the human can paste it |

### 5.2 Response shape & token discipline

Per clip: `id`, `content`, `content_type` (`text`/`code`/`url`/`image`),
`source`, `created_at` (ISO 8601), `byte_size`.

- `search`/`list` results **truncate** `content` to a preview to avoid
  blowing the AI context window; full content via `get_clipboard_item`.
- Image clips return metadata (+ `media_path`), never raw bytes.

### 5.3 Privacy & guards

- Reads only the local decrypted store; never touches the relay or the key.
- Clipboard capture already skips password managers and concealed
  pasteboard types, so secrets are largely never stored.
- **Read-only by default.** The write tool (`set_clipboard`) is exposed
  only when started with `cinch mcp --allow-write` (or equivalent config).
- **Local clipboard only** in v1 — no silent cross-device push.
- **Size cap** on written content (reject oversized payloads).
- **Visibility (nice-to-have):** if the desktop app is running, surface a
  notification ("Cinch: AI set your clipboard") so the human always knows.
- Future: an exposure-scope option (e.g., "last N days only", exclude
  certain sources/content types).

### 5.4 Code location & dependencies

- New subcommand: `crates/cli/src/commands/mcp.rs` + a JSON-RPC/stdio layer.
- Wraps existing `client_core::store::queries::{search_clips, list_clips}`.
- MCP protocol: evaluate the Rust SDK `rmcp` vs a minimal hand-rolled
  JSON-RPC-over-stdio (decide in planning).
- Clipboard **write** for the CLI does not exist yet (only the desktop has
  NSPasteboard write). Add it via `arboard` or by sharing the desktop's
  write path through `client_core` (decide in planning).

### 5.5 Integration (dogfooding path)

- Claude Code: `claude mcp add cinch -- cinch mcp` (or `.mcp.json`).
- Cursor: register `cinch mcp` in `mcp.json`.
- Future (optional): a desktop "Copy MCP config" helper.

## 6. Edge cases

- `store.db` missing / empty → empty results with a clear message.
- Offline clips (`synced = 0`) are included (correct — they are real local
  clips).
- Concurrent access with desktop/CLI — reads are safe (SQLite WAL); the
  existing `~/.cinch/sync.lock` coordinates writers.
- `set_clipboard` from the CLI will be captured by the desktop monitor as a
  new clip — acceptable and desirable ("AI set this" becomes recorded).

## 7. Out of scope (future)

- Semantic/embedding search (FTS5 keyword search is the v1 recall).
- Transform-on-recall (KO↔EN / summarize / to-table) — strong v2 candidate;
  hits the maintainer's personal bilingual pain and is the most "AI-era"
  hook, but adds AI calls and UI surface.
- Cross-device push / device targeting via MCP write.
- Smart ranking of recall (recency × frequency × pinned × current app).
- Desktop UI for MCP onboarding.

## 8. Success criteria

1. With `cinch` added to Claude Code, *"fix the error I just copied"* and
   *"pull the API spec I copied last week"* work **without manual
   copy/paste**.
2. In the maintainer's own dogfooding, the number of manual copy/pastes
   done purely to carry context to an AI drops noticeably.
3. With `--allow-write`, an agent can place a result on the local clipboard
   for the human to paste, and the human is never surprised by it.

## 9. Open questions for planning

- `rmcp` SDK vs hand-rolled JSON-RPC/stdio.
- CLI clipboard-write implementation (`arboard` vs shared `client_core`).
- Preview truncation length and `byte_size` thresholds.
- Exact `--allow-write` ergonomics (flag vs config vs both).
