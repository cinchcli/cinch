# Auto-copy the agent "resume" command on session exit

- **Date:** 2026-06-15
- **Status:** Approved design — ready for implementation plan
- **Components touched:** `crates/client-core`, `crates/cli`, `apps/desktop` (Rust + React)

## 1. Problem & motivation

When a coding-agent session ends, the agent CLI *prints* a resume command but
never puts it on the clipboard:

- Claude Code, on exit: `Resume this session with: claude --resume <uuid>`
- Codex, on interrupt: `To continue this session, run codex resume <uuid>`

To actually resume, the user has to hand-select that line out of the terminal
and copy it. cinch is a clipboard tool — it should capture the resume command
automatically when the session ends, drop it on the clipboard, and keep it in
clip history so it can be found later.

## 2. Goals / non-goals

**Goals**
- When a Claude Code **or** Codex session ends, automatically place the
  agent's resume command on the system clipboard.
- Also store it as a **local** clip in cinch history (source `claude` / `codex`).
- Make it toggleable on/off **per agent** from desktop Settings → *Agents & CLI*.
- Turning a toggle on **auto-installs** the necessary hook/wiring; turning it
  off removes it. Installation is idempotent and removable.

**Non-goals**
- Syncing the resume command to other devices. A resume command only works on
  the machine that owns the session, so it stays local (never pushed to the
  relay).
- Supporting agents beyond Claude Code and Codex in this iteration.
- A standalone "resume picker" UI. We only capture the just-ended session's
  command; browsing past sessions is out of scope.

## 3. User-facing behavior

### 3.1 Claude Code (native `SessionEnd` hook)

Claude Code exposes a first-class `SessionEnd` hook (verified against the
current docs). When enabled, cinch installs a hook in `~/.claude/settings.json`
that runs `cinch agent-hook claude-session-end`. On session end Claude pipes a
JSON object to the command's **stdin**:

```json
{
  "session_id": "47f6ad0f-f4e0-4136-8378-96b03100e385",
  "transcript_path": "/Users/.../.claude/projects/.../<session_id>.jsonl",
  "cwd": "/Users/...",
  "hook_event_name": "SessionEnd",
  "reason": "prompt_input_exit"
}
```

cinch builds `claude --resume <session_id>`, saves it as a local clip, and
writes it to the clipboard.

**Reason filtering.** `reason` is one of `clear`, `resume`, `logout`,
`prompt_input_exit`, `bypass_permissions_disabled`, `other`. We copy only on
`prompt_input_exit` and `other` (genuine "I'm leaving" exits) and **skip**
`clear`, `resume`, `logout`, `bypass_permissions_disabled`. This mirrors when
Claude itself prints the resume hint and avoids copying during `/clear` or
session switching. We register the hook **without** a `matcher` (fires on all
reasons) and do the filtering in cinch code so the policy is unit-testable.

### 3.2 Codex (shell wrapper — no native session-end event)

Verified against `openai/codex` source: Codex has **no** session-end / exit
event. Its `notify` mechanism emits only `agent-turn-complete`, and its hooks
system tops out at `Stop` (turn-scoped, like Claude's `Stop`). The only trigger
that fires on real exit/interrupt is a shell wrapper.

When enabled, cinch installs a guarded `codex()` function into the user's shell
rc:

```sh
# >>> cinch agent-resume (codex) >>>
codex() { local _s=$(date +%s); command codex "$@"; local _r=$?; command cinch agent-hook codex-exit --since "$_s" >/dev/null 2>&1; return $_r; }
# <<< cinch agent-resume (codex) <<<
```

- `command codex` / `command cinch` bypass the function so the real binaries run.
- On exit, `cinch agent-hook codex-exit --since <start>` finds the just-used
  session UUID from the newest `~/.codex/sessions/**/rollout-*.jsonl` file,
  builds `codex resume <uuid>`, and does the same local-save + clipboard write.
- `--since` is the wrapper's start timestamp; the command only acts if a rollout
  file was modified at/after that time, so `codex --version` (which writes no
  session) doesn't copy a stale command.

The user must restart their terminal (or `source` the rc) for the wrapper to
take effect; the UI says so.

### 3.3 Where the command lands

For both agents: the command is saved as a **local** clip (`SyncState::Local`,
source `claude` / `codex`) **and** written to the system clipboard. It appears
in clip history but is never synced to the relay.

## 4. Architecture & data flow

### 4.1 Why this produces one local clip and never syncs

The desktop clipboard monitor already has the two properties we need
(`apps/desktop/src-tauri/src/clipboard/monitor.rs`):

1. **It never contacts the relay** — the poller only does `capture_local`
   (`monitor.rs:4`). A captured clipboard echo can't accidentally sync.
2. **Cross-process echo guard** — `recent_store_duplicate_id` →
   `queries::recent_clip_id_by_content` (`monitor.rs:182-197`), built for
   `cinch session copy`: when a cinch process saves a clip to the shared store
   and then writes the same bytes to the clipboard, the poller finds the
   just-saved clip (within a 5s window) and **surfaces it instead of inserting
   a duplicate**.

Our hook reuses that exact pattern. Ordering is **insert-then-copy**:

```
agent exits
  → cinch agent-hook {claude-session-end | codex-exit}
      1. check setting agent_resume:<agent>; no-op if disabled
      2. resolve session id → build resume command
      3. insert LOCAL clip into ~/.cinch/store.db (source claude/codex, SyncState::Local)
      4. write the command to the system clipboard (arboard)
desktop poller (if running) sees the clipboard echo
  → recent_clip_id_by_content finds the clip from step 3
  → surfaces it (ClipReceived for the existing id); no 2nd clip; never pushes to relay
```

Net result: exactly one local clip, on the clipboard, in history, unsynced —
whether or not the desktop app is running.

### 4.2 Module layout (designed for isolation)

**`client_core::agent_resume` (new module).** Pure logic + filesystem/JSON only,
**no clipboard dependency**, so both the CLI and the desktop backend can call
it. Responsibilities:

- `enum Agent { Claude, Codex }` (serde + specta), with `as_str()` ↔ parse.
- `resume_command(agent, session_id) -> String`
  - Claude → `claude --resume <id>`
  - Codex → `codex resume <id>`
- `parse_claude_session_end(stdin_json: &str) -> Option<ClaudeSessionEnd>`
  (`{ session_id, reason }`) and `should_copy(reason: &str) -> bool`
  (true for `prompt_input_exit` / `other`).
- `latest_codex_session_id(codex_home: &Path, since: Option<i64>) -> Option<String>`
  — recursive glob of `rollout-*.jsonl` under `<codex_home>/sessions/`
  (handles both `YYYY/MM/DD/` and legacy flat layout), newest by filename
  timestamp / mtime, extract trailing UUID, require mtime ≥ `since` when given.
  `codex_home` = `$CODEX_HOME` or `~/.codex`.
- Install / uninstall / status:
  - **Claude:** `install_claude_hook`, `uninstall_claude_hook`,
    `is_claude_hook_installed` — idempotent edit of `~/.claude/settings.json`
    via `serde_json::Value` (preserve every other key and every other hook;
    our entry is matched by the command marker substring
    `agent-hook claude-session-end`). Create the file as `{}` if absent.
  - **Codex:** `install_codex_wrapper`, `uninstall_codex_wrapper`,
    `is_codex_wrapper_installed` — guarded marker block
    (`# >>> cinch agent-resume (codex) >>>` … `# <<< … <<<`) in the target
    shell rc. zsh/bash share the POSIX function syntax above; **fish** uses a
    different function syntax and is returned as a copy-paste snippet rather
    than auto-edited (see §6).

**`crates/cli` — hidden `agent-hook` command group**
(`crates/cli/src/commands/agent_hook.rs`). Owns the clipboard + store writes
(arboard lives in the CLI layer, matching current layering):

- `claude-session-end` *(hidden, machine-invoked)* — read stdin JSON →
  `parse_claude_session_end` → `should_copy` → save local clip + clipboard.
- `codex-exit --since <unix> [--cwd <path>]` *(hidden, machine-invoked)* —
  `latest_codex_session_id` → save local clip + clipboard.
- `enable <claude|codex>` / `disable <claude|codex>` / `status` *(user-facing
  thin wrappers)* — set the setting + install/uninstall via `agent_resume`, for
  CLI-only users and for testing. Desktop toggle remains the primary UX.

The local-clip save reuses the same local-capture path as `cinch copy`
(`queries::insert_clip` with `SyncState::Local`), content type via
`client_core::classify::detect`.

**`client_core::store::settings` — new keys**

- `agent_resume:claude`, `agent_resume:codex` → `"true"`/`"false"`, default
  `false`.
- Helpers mirroring the existing `is_source_auto_copy` / `set_source_auto_copy`
  pair: `is_agent_resume_enabled(store, agent)` /
  `set_agent_resume_enabled(store, agent, bool)`.
- The hidden hook commands read this and no-op when disabled (defense against a
  stale install left in a config file).

**`apps/desktop` — Tauri commands + UI**

- New commands (regenerate `bindings.ts` via
  `cargo test export_bindings -- --ignored`):
  - `get_agent_resume_config() -> AgentResumeConfig`
    `{ claude_enabled, codex_enabled, claude_installed, codex_installed }`
    (`*_installed` lets the UI flag drift if a user hand-removed the wiring).
  - `set_agent_resume_enabled(agent: Agent, enabled: bool) -> AgentResumeResult`
    `{ needs_shell_restart: bool, files_modified: Vec<String>, manual_snippet: Option<String> }`
    — flips the setting **and** installs/uninstalls.
- UI: a "Copy resume command on exit" subsection in the **Agents & CLI** tab
  (`components/AgentsSection.tsx`), two toggle rows reusing the existing
  checkbox style (`SettingsPane` `S.checkboxRow` / `S.checkBox`). On Codex
  enable, show the returned `files_modified` and a "Restart your terminal (or
  run `source ~/.zshrc`) to apply" note; if `manual_snippet` is present (fish),
  render it in a code block to paste manually.

### 4.3 Install-time command path

The hook/wrapper invoke `cinch …`. We write **bare `cinch`** and rely on PATH —
agent terminals already have the Homebrew `cinch` symlink on PATH, and the
symlink (not the in-bundle `Cinch` binary) is what routes to CLI dispatch via
`argv[0]`. (Calling the in-bundle binary directly would launch the GUI.)
Optional hardening, deferred: resolve the absolute symlink path at install time
with a bare-`cinch` fallback.

## 5. Testing strategy

- **`client_core::agent_resume` unit tests:**
  - `resume_command` strings for both agents.
  - `parse_claude_session_end` happy path + malformed JSON → `None`.
  - `should_copy` truth table over all six `reason` values.
  - `latest_codex_session_id`: temp dir with several `rollout-*.jsonl` across
    `YYYY/MM/DD/` and flat layouts → returns newest UUID; `since` filter
    excludes older files; empty dir → `None`; `$CODEX_HOME` override.
  - Claude install/uninstall: starts from missing file, `{}`, and a file with
    unrelated hooks → our entry added once (idempotent on re-install), other
    content preserved, uninstall restores prior shape and prunes empty groups.
  - Codex install/uninstall: marker block added once, idempotent, removed
    cleanly; surrounding rc content untouched.
- **`settings` tests:** `agent_resume:*` defaults false and round-trips
  (follow the existing `source_auto_copy_defaults_false_and_toggles` test).
- **CLI tests:** `claude-session-end` reads a fixture stdin JSON and, with the
  setting enabled, inserts one local clip with source `claude` and the expected
  content; disabled → no clip. `codex-exit` against a temp `CODEX_HOME`.
- **Desktop:** the echo-guard path is already covered by the monitor's
  `recent_store_duplicate_id` tests; add a `SettingsPane`/`AgentsSection` test
  for the toggle rows (mirroring `SettingsPane.test.tsx`).

## 6. Decisions

1. **Per-agent toggles** (two switches), not one master — Claude's install is a
   clean JSON edit, Codex's edits the shell rc; the user may want one but not
   the other.
2. **Codex shell scope:** auto-edit the rc for the default `$SHELL`
   (zsh → `~/.zshrc`; bash → `~/.bashrc` if present). **fish** gets a
   copy-paste snippet rather than an auto-edit (different function syntax).
3. **Claude reason filter:** copy on `prompt_input_exit` / `other`; skip the
   rest.
4. **Stable command names:** `cinch agent-hook claude-session-end` and
   `cinch agent-hook codex-exit` are baked into config files, so they are
   permanent strings.
5. **Hook binary path:** bare `cinch` (PATH-resolved); absolute-path hardening
   deferred.

## 7. Rejected alternatives

- **Codex `Stop` hook** — fires after every turn, so it would clobber the
  clipboard continuously during a session. Wrong granularity.
- **Filesystem watcher on the sessions dir** — no clean "ended" signal; "no
  writes for N seconds" is heuristic and racy.
- **Let the desktop poller capture the printed command** — the agents never put
  it on the clipboard, so there is nothing to capture without the hook/wrapper.

## 8. Edge cases & notes

- **Hard kill** (SIGKILL) of either agent won't fire the hook/wrapper — same
  limitation as the agents' own printed hint. Acceptable.
- **Desktop not running:** the hook still saves the local clip and sets the
  clipboard directly (the store at `~/.cinch/store.db` is shared); no echo
  guard is needed because there's no poller.
- **Multiple concurrent Codex sessions:** `--since` + newest-mtime picks the
  one that just ended; the rare ambiguous case is acceptable for v1.
- **Toggle drift:** if a user hand-removes the wiring, `*_installed` in
  `get_agent_resume_config` lets the UI show the toggle as on-but-not-installed
  and offer a re-install.

## 9. Post-review hardening

An adversarial review after implementation surfaced eight confirmed findings.
Six were fixed (each with a regression test); two were deliberately left as
documented limitations.

**Fixed**

- **rc removal must fail safe (HIGH).** `rc_without_codex_block` now strips only
  a *well-formed* `START..END` range. A dangling `START` (torn write, hand-trim,
  partial restore) no longer swallows the rest of `~/.zshrc`/`~/.bashrc`; the
  buffered lines are restored instead. This is the one data-loss bug — it
  removes the consequence of any torn write, which is why atomic-write was not
  also needed (see below).
- **Deterministic newest-session pick (MEDIUM).** `latest_codex_session_id`
  keeps sub-second mtime precision (nanoseconds, not truncated seconds) and adds
  a deterministic tie-break: on equal mtimes the lexicographically-larger
  rollout filename (sortable ISO-timestamp prefix) wins, so the result no longer
  depends on `read_dir` order.
- **Drift hint suppressed on manual shells (MEDIUM).** `AgentResumeConfig` gained
  `codex_manual_shell`; the desktop no longer shows "toggle off and on to
  reinstall" for fish/unknown shells, where Codex is never auto-installable and
  that advice is a dead end.
- **settings.json key order preserved (LOW).** `serde_json` now uses
  `preserve_order`, so editing the Claude hook no longer re-sorts the user's
  hand-maintained keys.
- **Bounded session walk (LOW).** `collect_rollouts` skips symlinked entries
  (via `file_type()`, which doesn't follow links) and caps recursion depth, so a
  symlink cycle under `sessions/` can't spin or exhaust the stack.
- **Load failure is visible (LOW).** A failed `get_agent_resume_config` now
  surfaces an error in Settings instead of silently rendering every toggle off
  (which would mask a genuinely-enabled config — the opposite of the drift
  intent).

**Deliberately not fixed**

- **Atomic config writes (LOW).** A naive temp-file + rename would *replace* a
  symlinked rc file (stow/dotbot/chezmoi) with a regular file — a new
  regression. With the rc-removal fix above, a torn write can no longer cause
  data loss, and plain `std::fs::write` matches the codebase's own `Config::save`
  convention. Revisit only with symlink-preserving atomic writes.
- **`--since` same-second boundary (LOW).** The Codex wrapper's start time comes
  from `date +%s` (whole seconds; macOS BSD `date` has no `%N`, and the default
  `/bin/bash` 3.2 has no `$EPOCHREALTIME`), so a stale rollout last modified in
  the same integer second the wrapper started can still pass the inclusive
  filter. The sub-second mtime fix above makes the *selection* deterministic;
  the residual sub-1-second window is inherent to a portable shell start stamp
  and is the basis for the LOW rating.
```
