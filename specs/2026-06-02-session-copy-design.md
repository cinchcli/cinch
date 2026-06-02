# `cinch session copy` — Design Spec

Date: 2026-06-02
Status: Approved (brainstorm) → implementation
Scope: cinch monorepo (`crates/cli` + `crates/client-core`). No relay change.
Test platform: macOS only (first cut).

## Problem

When working in an agent coding session (Claude Code today; codex / gemini-cli
later), the user sometimes wants to grab a **specific answer** out of that
session and carry it elsewhere — paste it into another AI/agent for a second
opinion or continuation, move it to another machine, or keep it for later
search/reference.

Plain `Cmd-C` is poor for this: the session lives as a noisy JSONL transcript
(`~/.claude/projects/<encoded-cwd>/<uuid>.jsonl`) mixing user/assistant text
with tool calls, tool results, thinking blocks, and base64 attachments. You
cannot cleanly select "that one answer" by hand, and the raw text is unusable.

cinch is positioned to add real value over plain copy because the captured
content lands in cinch's clip pipeline: it **syncs across devices**, becomes
**searchable (FTS)**, and is **immediately on the system clipboard** for paste.

## Goal

A user-triggered command that lets you:

1. **Select a session** (default: the most recent session of the current
   working directory's project; or by id prefix; or via an interactive picker).
2. **Select which answer(s)** within that session to copy (interactive picker /
   `--last [N]` / `--all`).
3. Render the selection to **clean Markdown** and **both** save it as a cinch
   clip and place it on the system clipboard.

Non-goals (this cut): the AI-distilled "handoff packet" (Phase 2), codex /
gemini-cli source readers (structure for it, don't build it), Windows/Linux
testing (cross-platform code is fine; only macOS is validated now), a fancy
full-screen TUI (a numbered-list + stdin selection is the picker MVP).

## Definitions

- **Answer (unit of selection):** one full assistant response to a single user
  prompt — i.e. everything from a real user prompt up to (but not including) the
  next real user prompt. This spans possibly multiple assistant records with
  interleaved tool-use / tool-result steps, ending at the assistant's final
  text. This is the unit the user selects and copies. (Confirmed with user.)
- **Session:** one `*.jsonl` transcript file.
- **Source:** the agent tool that produced the session. `claude` now;
  `codex` / `gemini` are future trait implementations.

## Command surface

```
cinch session copy [SESSION] [--from claude]

  SESSION              session id prefix | "latest" (default) | (with --pick) interactive
  --from <source>      session source. default & only value now: "claude"
  --pick               interactively choose the SESSION too (not just the answer)

  # answer selection (mutually exclusive; default = interactive answer picker):
  --last [N]           last N answers (default N=1). non-interactive → slash-command default
  --all                whole session (every answer, in order)

  # options:
  --with-prompt        include the eliciting user prompt above each answer
  --include-thinking   include assistant thinking blocks (default: off — noise for handoff)
  --no-tools           exclude tool calls/results (default: include, results truncated)
  --stdout             write Markdown to stdout instead of saving a clip
  --no-copy            skip the system-clipboard copy
  -l, --label <text>   clip label (default: derived session/answer title)
```

`--from` defaults to `claude`. Bare `cinch session copy` ⇒ source=claude,
session=latest-in-cwd, answer=interactive picker.

### Selection model (two levels)

1. **Session resolution.** Map `cwd` → Claude's encoded project dir name
   (replace `/` with `-`, leading `-`), i.e.
   `~/.claude/projects/-Users-...-relay-main/`. Pick the most recently modified
   `*.jsonl` by default. `--pick` shows a session list (title + mtime +
   id-prefix); a positional `SESSION` matches an id prefix.
2. **Answer resolution.** Parse the chosen session into an ordered list of
   answers. Default: interactive picker (numbered list with a one-line preview
   per answer; user types one or more numbers / a range). `--last [N]` and
   `--all` are the non-interactive paths. Slash command uses `--last`.

Interactive picker requires a TTY; if stdin is not a TTY and no
`--last`/`--all` was given, error with a clear message pointing at `--last`.

## Architecture

### `client_core::session` (new, reusable — desktop can consume later)

A small module, source-agnostic:

- `model.rs` — plain data types:
  - `Session { id, title: Option<String>, path, answers: Vec<Answer> }`
  - `Answer { index, prompt: Option<Prompt>, parts: Vec<AnswerPart> }`
  - `AnswerPart` enum: `Text(String)`, `ToolUse { name, input }`,
    `ToolResult { truncated_text }`, `Thinking(String)`,
    `Attachment { label }`
- `source.rs` — `trait SessionSource` with:
  - `list_sessions(cwd) -> Vec<SessionRef>` (id, title, mtime)
  - `load(session_ref_or_latest) -> Session`
  - `ClaudeSource` is the first impl. It owns the cwd→encoded-path logic and
    the JSONL record/`message.content` parsing, grouping records into `Answer`s.
- `render.rs` — `markdown(answers: &[Answer], opts: RenderOpts) -> String`.
  `RenderOpts { with_prompt, include_thinking, include_tools, tool_result_max }`.
  Renders `## User` / `## Assistant`, tool steps as fenced `tool: <name>`
  blocks, truncates tool results, replaces attachments/base64/file-snapshots
  with `[attachment: <label>]` placeholders.

Errors via the crate's existing error convention (match `transform.rs` /
`store`). Unit tests live next to the module with a small fixture JSONL
exercising every record/content shape and the answer-grouping boundary.

### `crates/cli/src/commands/session.rs` (new)

- clap `Args` (subcommand group `session` with `copy` for now), registered in
  `commands/mod.rs` and the top-level `Cli` enum following the `push` / `ai` /
  `transform` patterns.
- Flow: resolve source → resolve session → parse → resolve answer selection
  (picker via stdin / `--last` / `--all`) → `render::markdown` → then:
  - unless `--stdout`: save as a cinch clip (reuse the `push` storage path —
    `StoredClip`, `content_type = text`, `SyncState` pending so it syncs) with
    the derived/`-l` label;
  - unless `--no-copy`: copy the Markdown to the system clipboard via the
    existing `crate::io::copy_text_to_clipboard` helper;
  - `--stdout`: write Markdown to stdout (`crate::io::write_to_stdout`), skip
    clip+clipboard.
- Exit/error handling via `crate::exit`.

### Slash command — `.claude/commands/cinch-copy.md`

A thin Claude Code slash command that runs, non-interactively:
`cinch session copy --from claude --last $ARGUMENTS`
(default: last answer). Lets the user trigger from inside a session.

## Value over plain copy (why this lands in cinch)

Saving as a clip + clipboard simultaneously is the whole point:
1. **Paste anywhere now** — clipboard has clean Markdown for another AI/agent.
2. **Cross-device** — the clip syncs to other machines automatically.
3. **Searchable archive** — FTS over clip history for later reference.

## Phases

- **Phase 1 (now, macOS):** everything above = local full-clean render. No AI.
- **Phase 2 (later):** `--format handoff` → distilled "resume packet" via the
  existing `cinch ai` surface (goal / decisions / current state / files touched
  / next steps). Built only after Phase 1 is validated.

## Testing

- **Unit:** `client_core::session` parser + renderer against a fixture JSONL
  (covers text, tool_use, tool_result, thinking, attachment, multi-record
  answers, answer boundaries, title extraction, cwd→encoded-path).
- **CLI:** selection logic (`--last N`, `--all`, non-TTY error path), label
  derivation, `--stdout` vs clip+clipboard branching.
- **Manual (macOS):** in this repo, `cinch session copy --last 1` → confirm a
  clip is created (`cinch list`/desktop), clipboard holds clean Markdown, and it
  syncs to another device. Interactive picker selects a mid-session answer.
- Gates: `cargo build --workspace`, `cargo test`, `cargo clippy`, `cargo fmt`.

## Open / deferred

- Multi-answer concatenation ordering: render selected answers in session order.
- Exact picker keybindings: MVP is numbered list + stdin line; richer TUI later.
- Agent-neutral naming is locked in (`session` group, `--from`), even though
  only `claude` ships now.
