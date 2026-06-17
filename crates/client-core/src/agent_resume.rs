//! Auto-copy an agent's "resume this session" command when a coding-agent
//! session ends.
//!
//! This module is the pure / filesystem-only core shared by the CLI (the
//! hidden `cinch agent-hook` entrypoints) and the desktop settings toggle. It
//! has **no clipboard or store dependency** — building the resume command,
//! parsing the agent's hook payload, discovering the just-used session, and
//! installing/removing the per-agent wiring all live here so both consumers
//! call one tested implementation.
//!
//! Two agents, two very different triggers:
//!
//! - **Claude Code** has a native `SessionEnd` hook. We install a hook into
//!   `~/.claude/settings.json` that pipes a JSON payload (carrying
//!   `session_id` + `reason`) to `cinch agent-hook claude-session-end`.
//! - **Codex** has *no* session-end event (verified against `openai/codex`:
//!   its `notify` emits only `agent-turn-complete` and its hooks top out at the
//!   turn-scoped `Stop`). The only thing that fires on real exit is a shell
//!   wrapper, so we install a guarded `codex()` function into the shell rc that
//!   calls `cinch agent-hook codex-exit` after the real `codex` returns; that
//!   entrypoint recovers the just-used session UUID from the newest
//!   `~/.codex/sessions/**/rollout-*.jsonl` file.

use std::path::{Path, PathBuf};

#[cfg(feature = "specta")]
use specta::Type;

/// A supported coding agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(Type))]
#[serde(rename_all = "lowercase")]
pub enum Agent {
    Claude,
    Codex,
}

impl Agent {
    pub fn as_str(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        }
    }

    /// Parse the lowercase wire string (`"claude"` / `"codex"`).
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "claude" => Some(Agent::Claude),
            "codex" => Some(Agent::Codex),
            _ => None,
        }
    }
}

/// The fields we care about from Claude Code's `SessionEnd` hook payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeSessionEnd {
    pub session_id: String,
    pub reason: String,
}

/// Errors from the install/uninstall filesystem operations.
#[derive(Debug, thiserror::Error)]
pub enum AgentResumeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Claude settings.json is not a JSON object")]
    NotAnObject,
}

/// Marker substring identifying *our* Claude hook entry, so install is
/// idempotent and uninstall can find it even if the resolved binary path
/// differs from the default.
pub const CLAUDE_HOOK_MARKER: &str = "agent-hook claude-session-end";

/// The default command we register as the Claude `SessionEnd` hook. Relies on
/// the PATH `cinch` launcher (the symlink that routes to CLI dispatch via
/// argv[0]); the in-bundle `Cinch` binary must NOT be called directly.
pub const DEFAULT_CLAUDE_HOOK_COMMAND: &str = "cinch agent-hook claude-session-end";

/// Default binary token used inside the Codex shell wrapper.
pub const DEFAULT_CINCH_BIN: &str = "cinch";

/// Guard markers bracketing the Codex shell-wrapper block in the rc file.
pub const CODEX_BLOCK_START: &str = "# >>> cinch agent-resume (codex) >>>";
pub const CODEX_BLOCK_END: &str = "# <<< cinch agent-resume (codex) <<<";

// ── Resume command ──────────────────────────────────────────────────────────

/// Build the agent's resume command line for a given session id.
pub fn resume_command(agent: Agent, session_id: &str) -> String {
    match agent {
        Agent::Claude => format!("claude --resume {session_id}"),
        Agent::Codex => format!("codex resume {session_id}"),
    }
}

// ── Claude SessionEnd payload ─────────────────────────────────────────────────

/// Parse the Claude `SessionEnd` hook stdin JSON. Returns `None` for malformed
/// JSON or a missing/empty `session_id`. A missing `reason` defaults to
/// `"other"` (the catch-all termination reason).
pub fn parse_claude_session_end(json: &str) -> Option<ClaudeSessionEnd> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let session_id = v.get("session_id")?.as_str()?.to_string();
    if session_id.is_empty() {
        return None;
    }
    let reason = v
        .get("reason")
        .and_then(|r| r.as_str())
        .unwrap_or("other")
        .to_string();
    Some(ClaudeSessionEnd { session_id, reason })
}

/// Whether to copy the resume command for a given `SessionEnd` reason. We copy
/// only on genuine "the user is leaving" exits and skip `/clear`, session
/// switching (`resume`), `logout`, and `bypass_permissions_disabled` — the same
/// cases where Claude itself does not print the resume hint.
pub fn should_copy_for_reason(reason: &str) -> bool {
    matches!(reason, "prompt_input_exit" | "other")
}

// ── Codex session discovery ───────────────────────────────────────────────────

/// `$CODEX_HOME` or `~/.codex`.
pub fn codex_home() -> PathBuf {
    if let Some(v) = std::env::var_os("CODEX_HOME") {
        return PathBuf::from(v);
    }
    dirs::home_dir().unwrap_or_default().join(".codex")
}

/// Extract the trailing UUID from a `rollout-<ts>-<uuid>.jsonl` filename.
pub fn parse_rollout_uuid(filename: &str) -> Option<String> {
    let stem = filename.strip_suffix(".jsonl")?;
    if !stem.starts_with("rollout-") || stem.len() < 37 {
        return None;
    }
    // The UUID is the trailing 36 chars; the char before it must be '-'.
    let uuid = &stem[stem.len() - 36..];
    if stem.as_bytes().get(stem.len() - 37) != Some(&b'-') {
        return None;
    }
    if is_uuid(uuid) {
        Some(uuid.to_string())
    } else {
        None
    }
}

fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    for (i, c) in b.iter().enumerate() {
        match i {
            8 | 13 | 18 | 23 => {
                if *c != b'-' {
                    return false;
                }
            }
            _ => {
                if !c.is_ascii_hexdigit() {
                    return false;
                }
            }
        }
    }
    true
}

fn is_rollout_file(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.starts_with("rollout-") && n.ends_with(".jsonl"))
        .unwrap_or(false)
}

/// File mtime as whole nanoseconds since the epoch. Sub-second precision is
/// kept so two rollouts written in the same wall-clock second still compare
/// correctly (truncating to seconds would make them a false tie).
fn mtime_nanos(p: &Path) -> Option<u128> {
    let modified = std::fs::metadata(p).ok()?.modified().ok()?;
    let d = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(d.as_nanos())
}

/// The real codex layout is at most `sessions/YYYY/MM/DD/` (3 levels). This cap
/// bounds the recursive walk so a symlink cycle or pathological nesting under
/// `sessions/` can never spin or exhaust the stack.
const MAX_ROLLOUT_DEPTH: u32 = 8;

fn collect_rollouts(dir: &Path, depth: u32, out: &mut Vec<(PathBuf, u128)>) {
    if depth > MAX_ROLLOUT_DEPTH {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        // `file_type()` does NOT follow symlinks, so a symlinked directory is
        // neither `is_dir()` nor `is_file()` and is skipped entirely — no cycle
        // to descend.
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        let p = entry.path();
        if ft.is_dir() {
            collect_rollouts(&p, depth + 1, out);
        } else if ft.is_file() && is_rollout_file(&p) {
            if let Some(mt) = mtime_nanos(&p) {
                out.push((p, mt));
            }
        }
    }
}

/// The session UUID of the newest `rollout-*.jsonl` under `<codex_home>/sessions/`
/// (recursively; handles both the `YYYY/MM/DD/` layout and the legacy flat
/// one). When `since` (unix seconds) is given, only files modified at/after
/// that time are considered — so a `codex` invocation that wrote no session
/// (e.g. `codex --version`) doesn't surface a stale command.
pub fn latest_codex_session_id(codex_home: &Path, since: Option<i64>) -> Option<String> {
    let sessions = codex_home.join("sessions");
    let mut files = Vec::new();
    collect_rollouts(&sessions, 0, &mut files);

    // `since` is the wrapper's whole-second start time; compare in nanoseconds
    // so both sides use the same unit.
    let since_nanos = since.map(|s| (s.max(0) as u128) * 1_000_000_000);

    // Track (mtime, filename, uuid). On equal mtimes the lexicographically
    // larger filename wins — rollout names carry a sortable ISO-timestamp
    // prefix, so the winner is the genuinely-newer session and is reproducible
    // regardless of the OS-defined read_dir order.
    let mut best: Option<(u128, String, String)> = None;
    for (path, mtime) in files {
        if let Some(t) = since_nanos {
            if mtime < t {
                continue;
            }
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(uuid) = parse_rollout_uuid(name) else {
            continue;
        };
        let better = match &best {
            Some((bm, bn, _)) => (mtime, name) > (*bm, bn.as_str()),
            None => true,
        };
        if better {
            best = Some((mtime, name.to_string(), uuid));
        }
    }
    best.map(|(_, _, uuid)| uuid)
}

// ── Claude hook install/uninstall (settings.json) ─────────────────────────────

/// `~/.claude/settings.json`.
pub fn claude_settings_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude").join("settings.json"))
}

/// Does a `SessionEnd` group array contain a hook whose command carries our
/// marker?
fn session_end_has_our_hook(arr: &[serde_json::Value]) -> bool {
    arr.iter().any(|group| {
        group
            .get("hooks")
            .and_then(|h| h.as_array())
            .is_some_and(|hooks| {
                hooks.iter().any(|h| {
                    h.get("command")
                        .and_then(|c| c.as_str())
                        .is_some_and(|c| c.contains(CLAUDE_HOOK_MARKER))
                })
            })
    })
}

/// Whether the given settings JSON string already contains our SessionEnd hook.
pub fn claude_hook_present_in(json: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(json) else {
        return false;
    };
    match v
        .get("hooks")
        .and_then(|h| h.get("SessionEnd"))
        .and_then(|s| s.as_array())
    {
        Some(arr) => session_end_has_our_hook(arr),
        None => false,
    }
}

/// Return the settings JSON with our `SessionEnd` hook merged in. Idempotent:
/// if our hook is already present the input is returned (re-serialized)
/// unchanged. All other keys and hooks are preserved.
pub fn claude_settings_with_hook(
    existing: Option<&str>,
    command: &str,
) -> Result<String, AgentResumeError> {
    let mut root: serde_json::Value = match existing {
        Some(s) if !s.trim().is_empty() => serde_json::from_str(s)?,
        _ => serde_json::json!({}),
    };
    let obj = root.as_object_mut().ok_or(AgentResumeError::NotAnObject)?;
    let hooks = obj.entry("hooks").or_insert_with(|| serde_json::json!({}));
    let hooks_obj = hooks.as_object_mut().ok_or(AgentResumeError::NotAnObject)?;
    let session_end = hooks_obj
        .entry("SessionEnd")
        .or_insert_with(|| serde_json::json!([]));
    let arr = session_end
        .as_array_mut()
        .ok_or(AgentResumeError::NotAnObject)?;
    if !session_end_has_our_hook(arr) {
        arr.push(serde_json::json!({
            "hooks": [ { "type": "command", "command": command } ]
        }));
    }
    Ok(serde_json::to_string_pretty(&root)?)
}

/// Return the settings JSON with our `SessionEnd` hook removed. Empty hook
/// groups, an emptied `SessionEnd` array, and an emptied `hooks` object are
/// pruned. Other content is preserved.
pub fn claude_settings_without_hook(existing: &str) -> Result<String, AgentResumeError> {
    let mut root: serde_json::Value = serde_json::from_str(existing)?;
    let mut hooks_now_empty = false;
    if let Some(hooks_obj) = root.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        let mut remove_session_end = false;
        if let Some(arr) = hooks_obj
            .get_mut("SessionEnd")
            .and_then(|s| s.as_array_mut())
        {
            arr.retain_mut(|group| {
                if let Some(hooks_arr) = group.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                    hooks_arr.retain(|h| {
                        !h.get("command")
                            .and_then(|c| c.as_str())
                            .is_some_and(|c| c.contains(CLAUDE_HOOK_MARKER))
                    });
                    !hooks_arr.is_empty()
                } else {
                    true
                }
            });
            remove_session_end = arr.is_empty();
        }
        if remove_session_end {
            hooks_obj.remove("SessionEnd");
        }
        hooks_now_empty = hooks_obj.is_empty();
    }
    if hooks_now_empty {
        if let Some(root_obj) = root.as_object_mut() {
            root_obj.remove("hooks");
        }
    }
    Ok(serde_json::to_string_pretty(&root)?)
}

pub fn install_claude_hook(settings_path: &Path, command: &str) -> Result<(), AgentResumeError> {
    let existing = std::fs::read_to_string(settings_path).ok();
    let updated = claude_settings_with_hook(existing.as_deref(), command)?;
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(settings_path, updated)?;
    Ok(())
}

pub fn uninstall_claude_hook(settings_path: &Path) -> Result<(), AgentResumeError> {
    let Some(existing) = std::fs::read_to_string(settings_path).ok() else {
        return Ok(());
    };
    let updated = claude_settings_without_hook(&existing)?;
    std::fs::write(settings_path, updated)?;
    Ok(())
}

pub fn is_claude_hook_installed(settings_path: &Path) -> bool {
    std::fs::read_to_string(settings_path)
        .ok()
        .is_some_and(|s| claude_hook_present_in(&s))
}

// ── Codex shell wrapper install/uninstall (rc file) ───────────────────────────

/// The POSIX (zsh/bash) `codex()` wrapper block, including guard markers.
pub fn codex_wrapper_block(bin: &str) -> String {
    format!(
        "{CODEX_BLOCK_START}\n\
         codex() {{ local _s=$(date +%s); command codex \"$@\"; local _r=$?; command {bin} agent-hook codex-exit --since \"$_s\" >/dev/null 2>&1; return $_r; }}\n\
         {CODEX_BLOCK_END}\n"
    )
}

/// The fish-shell equivalent, returned as a copy-paste snippet (fish is never
/// auto-edited because its function syntax differs).
pub fn codex_fish_snippet(bin: &str) -> String {
    format!(
        "{CODEX_BLOCK_START}\n\
         function codex\n    \
         set -l _s (date +%s)\n    \
         command codex $argv\n    \
         set -l _r $status\n    \
         command {bin} agent-hook codex-exit --since $_s >/dev/null 2>&1\n    \
         return $_r\n\
         end\n\
         {CODEX_BLOCK_END}\n"
    )
}

/// Add the Codex wrapper block to rc content if absent (idempotent).
pub fn rc_with_codex_block(existing: &str, bin: &str) -> String {
    if existing.contains(CODEX_BLOCK_START) {
        return existing.to_string();
    }
    let mut out = existing.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&codex_wrapper_block(bin));
    out
}

/// Remove the Codex wrapper block from rc content (idempotent: returns the
/// input unchanged when no block is present).
///
/// Only a **well-formed** `START..END` range is stripped. A dangling START
/// with no matching END (a torn write, a hand-trim, a partial restore) must
/// NOT swallow the rest of the file — the guard markers exist precisely to
/// tolerate hand edits, so an unmatched START fails safe: the buffered lines
/// (START included) are restored and the file's tail is preserved.
pub fn rc_without_codex_block(existing: &str) -> String {
    if !existing.contains(CODEX_BLOCK_START) {
        return existing.to_string();
    }
    let mut out = String::new();
    // Lines seen since an open START, held back until we know whether a
    // matching END arrives (then discard) or not (then flush back).
    let mut buffered: Vec<&str> = Vec::new();
    let mut in_block = false;
    for line in existing.lines() {
        let trimmed = line.trim();
        if !in_block && trimmed == CODEX_BLOCK_START {
            in_block = true;
            buffered.clear();
            buffered.push(line);
            continue;
        }
        if in_block {
            if trimmed == CODEX_BLOCK_END {
                // Complete block — drop the whole START..END range.
                in_block = false;
                buffered.clear();
            } else {
                buffered.push(line);
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    // Unterminated block: restore everything we held back.
    if in_block {
        for line in &buffered {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

pub fn install_codex_wrapper(rc_path: &Path, bin: &str) -> Result<(), AgentResumeError> {
    let existing = std::fs::read_to_string(rc_path).unwrap_or_default();
    let updated = rc_with_codex_block(&existing, bin);
    if let Some(parent) = rc_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(rc_path, updated)?;
    Ok(())
}

pub fn uninstall_codex_wrapper(rc_path: &Path) -> Result<(), AgentResumeError> {
    let Some(existing) = std::fs::read_to_string(rc_path).ok() else {
        return Ok(());
    };
    let updated = rc_without_codex_block(&existing);
    std::fs::write(rc_path, updated)?;
    Ok(())
}

pub fn is_codex_wrapper_installed(rc_path: &Path) -> bool {
    std::fs::read_to_string(rc_path)
        .map(|s| s.contains(CODEX_BLOCK_START))
        .unwrap_or(false)
}

// ── Shell detection ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Zsh,
    Bash,
    Fish,
    Other,
}

/// Classify a `$SHELL` value by basename.
pub fn shell_kind(shell_env: Option<&str>) -> ShellKind {
    let base = shell_env
        .map(|s| s.rsplit('/').next().unwrap_or(s))
        .unwrap_or("");
    if base.contains("zsh") {
        ShellKind::Zsh
    } else if base.contains("bash") {
        ShellKind::Bash
    } else if base.contains("fish") {
        ShellKind::Fish
    } else {
        ShellKind::Other
    }
}

/// The rc file we auto-edit for a POSIX shell (zsh → `~/.zshrc`, bash →
/// `~/.bashrc`). `None` for fish/other (handled via a manual snippet).
pub fn posix_rc_path(kind: ShellKind, home: &Path) -> Option<PathBuf> {
    match kind {
        ShellKind::Zsh => Some(home.join(".zshrc")),
        ShellKind::Bash => Some(home.join(".bashrc")),
        ShellKind::Fish | ShellKind::Other => None,
    }
}

/// How to install the Codex wrapper for a given shell — shared by the CLI and
/// the desktop toggle so both make the same auto-edit-vs-manual decision.
pub enum CodexTarget {
    /// A POSIX rc file we can auto-edit (zsh/bash).
    Posix(PathBuf),
    /// fish or an unknown shell — the caller surfaces this snippet for the
    /// user to paste manually (we never auto-edit fish's different syntax).
    Manual(String),
}

/// Resolve where/how to install the Codex wrapper from a `$SHELL` value, home
/// directory, and the cinch binary token to embed in the wrapper.
pub fn codex_target(shell_env: Option<&str>, home: &Path, bin: &str) -> CodexTarget {
    let kind = shell_kind(shell_env);
    match posix_rc_path(kind, home) {
        Some(rc) => CodexTarget::Posix(rc),
        None => {
            let snippet = match kind {
                ShellKind::Fish => codex_fish_snippet(bin),
                _ => codex_wrapper_block(bin),
            };
            CodexTarget::Manual(snippet)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Agent ────────────────────────────────────────────────────────────────

    #[test]
    fn agent_str_roundtrip() {
        assert_eq!(Agent::Claude.as_str(), "claude");
        assert_eq!(Agent::Codex.as_str(), "codex");
        assert_eq!(Agent::from_str_opt("claude"), Some(Agent::Claude));
        assert_eq!(Agent::from_str_opt("codex"), Some(Agent::Codex));
        assert_eq!(Agent::from_str_opt("gemini"), None);
        assert_eq!(Agent::from_str_opt(""), None);
    }

    // ── resume_command ─────────────────────────────────────────────────────────

    #[test]
    fn resume_command_per_agent() {
        let id = "47f6ad0f-f4e0-4136-8378-96b03100e385";
        assert_eq!(
            resume_command(Agent::Claude, id),
            format!("claude --resume {id}")
        );
        assert_eq!(
            resume_command(Agent::Codex, id),
            format!("codex resume {id}")
        );
    }

    // ── parse_claude_session_end + should_copy_for_reason ──────────────────────

    #[test]
    fn parse_claude_happy_path() {
        let json = r#"{
            "session_id": "abc-123",
            "transcript_path": "/x/abc-123.jsonl",
            "cwd": "/x",
            "hook_event_name": "SessionEnd",
            "reason": "prompt_input_exit"
        }"#;
        let got = parse_claude_session_end(json).unwrap();
        assert_eq!(got.session_id, "abc-123");
        assert_eq!(got.reason, "prompt_input_exit");
    }

    #[test]
    fn parse_claude_missing_reason_defaults_to_other() {
        let got = parse_claude_session_end(r#"{"session_id":"s1"}"#).unwrap();
        assert_eq!(got.reason, "other");
    }

    #[test]
    fn parse_claude_rejects_malformed_and_empty() {
        assert!(parse_claude_session_end("not json").is_none());
        assert!(parse_claude_session_end("{}").is_none());
        assert!(parse_claude_session_end(r#"{"session_id":""}"#).is_none());
        assert!(parse_claude_session_end(r#"{"session_id":123}"#).is_none());
    }

    #[test]
    fn should_copy_only_on_real_exit_reasons() {
        assert!(should_copy_for_reason("prompt_input_exit"));
        assert!(should_copy_for_reason("other"));
        for skip in ["clear", "resume", "logout", "bypass_permissions_disabled"] {
            assert!(!should_copy_for_reason(skip), "must skip {skip}");
        }
    }

    // ── parse_rollout_uuid ─────────────────────────────────────────────────────

    #[test]
    fn parse_rollout_uuid_extracts_trailing_uuid() {
        assert_eq!(
            parse_rollout_uuid(
                "rollout-2025-05-07T17-24-21-5973b6c0-94b8-487b-a530-2aeb6098ae0e.jsonl"
            ),
            Some("5973b6c0-94b8-487b-a530-2aeb6098ae0e".to_string())
        );
    }

    #[test]
    fn parse_rollout_uuid_rejects_non_matches() {
        assert!(parse_rollout_uuid("rollout-2025-05-07.jsonl").is_none());
        assert!(parse_rollout_uuid("notes.txt").is_none());
        assert!(parse_rollout_uuid("rollout-xxxx.jsonl").is_none());
        // No "rollout-" prefix.
        assert!(parse_rollout_uuid("5973b6c0-94b8-487b-a530-2aeb6098ae0e.jsonl").is_none());
    }

    // ── latest_codex_session_id ────────────────────────────────────────────────

    fn write_rollout(dir: &Path, uuid: &str, mtime_secs: i64) -> PathBuf {
        write_rollout_at(
            dir,
            "2025-05-07T17-24-21",
            uuid,
            std::time::Duration::from_secs(mtime_secs as u64),
        )
    }

    /// Like [`write_rollout`] but lets the test control the filename's ISO
    /// timestamp prefix and a sub-second mtime — needed to exercise the
    /// sub-second precision and the deterministic tie-break.
    fn write_rollout_at(dir: &Path, iso: &str, uuid: &str, mtime: std::time::Duration) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join(format!("rollout-{iso}-{uuid}.jsonl"));
        std::fs::write(&path, b"{}").unwrap();
        let mt = std::time::UNIX_EPOCH + mtime;
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(mt)
            .unwrap();
        path
    }

    #[test]
    fn latest_codex_session_picks_newest_by_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let sessions = home.join("sessions").join("2025").join("05").join("07");
        write_rollout(&sessions, "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa", 1000);
        write_rollout(&sessions, "bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb", 2000);
        assert_eq!(
            latest_codex_session_id(home, None).as_deref(),
            Some("bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb")
        );
    }

    #[test]
    fn latest_codex_session_respects_since_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let sessions = home.join("sessions");
        write_rollout(&sessions, "cccccccc-cccc-4ccc-cccc-cccccccccccc", 1000);
        // Nothing modified at/after 5000 → None.
        assert!(latest_codex_session_id(home, Some(5000)).is_none());
        // The file at 1000 is included when since <= 1000.
        assert_eq!(
            latest_codex_session_id(home, Some(1000)).as_deref(),
            Some("cccccccc-cccc-4ccc-cccc-cccccccccccc")
        );
    }

    #[test]
    fn latest_codex_session_none_when_empty() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(latest_codex_session_id(tmp.path(), None).is_none());
    }

    #[test]
    fn latest_codex_session_uses_subsecond_precision() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let sessions = home.join("sessions");
        // Both land in the same whole second; B is 0.9s newer. Truncating to
        // whole seconds would treat them as a tie, but B is genuinely newest.
        // B's ISO prefix is *smaller* so only sub-second precision (not the
        // filename tie-break) can pick it correctly.
        write_rollout_at(
            &sessions,
            "2025-05-07T00-00-09",
            "aaaaaaaa-aaaa-4aaa-aaaa-aaaaaaaaaaaa",
            std::time::Duration::from_millis(1_000_000),
        );
        write_rollout_at(
            &sessions,
            "2025-05-07T00-00-01",
            "bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb",
            std::time::Duration::from_millis(1_000_900),
        );
        assert_eq!(
            latest_codex_session_id(home, None).as_deref(),
            Some("bbbbbbbb-bbbb-4bbb-bbbb-bbbbbbbbbbbb")
        );
    }

    #[test]
    fn latest_codex_session_tiebreak_is_deterministic_on_equal_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let sessions = home.join("sessions");
        // Identical mtimes — the winner must be reproducible regardless of the
        // OS-defined read_dir order. The later ISO-timestamp prefix (which
        // sorts lexicographically largest) is the genuinely-newer session.
        let eq = std::time::Duration::from_secs(1000);
        write_rollout_at(
            &sessions,
            "2025-05-07T09-00-00",
            "11111111-1111-4111-1111-111111111111",
            eq,
        );
        write_rollout_at(
            &sessions,
            "2025-05-07T12-00-00",
            "22222222-2222-4222-2222-222222222222",
            eq,
        );
        write_rollout_at(
            &sessions,
            "2025-05-07T10-30-00",
            "33333333-3333-4333-3333-333333333333",
            eq,
        );
        assert_eq!(
            latest_codex_session_id(home, None).as_deref(),
            Some("22222222-2222-4222-2222-222222222222")
        );
    }

    #[test]
    fn latest_codex_session_ignores_implausibly_deep_nesting() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path();
        let sessions = home.join("sessions");
        // Real codex layout is shallow (YYYY/MM/DD). A normally-placed rollout:
        let shallow = sessions.join("2025").join("05").join("07");
        write_rollout(&shallow, "11111111-1111-4111-1111-111111111111", 1000);
        // A rollout buried far deeper than any real layout — this is what a
        // symlink cycle synthesizes — must be ignored, bounding the walk.
        let mut deep = sessions.clone();
        for _ in 0..14 {
            deep = deep.join("x");
        }
        write_rollout(&deep, "22222222-2222-4222-2222-222222222222", 5000);
        // The deep file is "newer" but out of bounds, so only the shallow one
        // (the real session) is considered.
        assert_eq!(
            latest_codex_session_id(home, None).as_deref(),
            Some("11111111-1111-4111-1111-111111111111")
        );
    }

    // ── Claude settings.json editing ───────────────────────────────────────────

    #[test]
    fn claude_with_hook_from_empty_creates_structure() {
        let out = claude_settings_with_hook(None, DEFAULT_CLAUDE_HOOK_COMMAND).unwrap();
        assert!(claude_hook_present_in(&out));
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let cmd = v["hooks"]["SessionEnd"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert_eq!(cmd, DEFAULT_CLAUDE_HOOK_COMMAND);
        assert_eq!(v["hooks"]["SessionEnd"][0]["hooks"][0]["type"], "command");
    }

    #[test]
    fn claude_with_hook_is_idempotent() {
        let once = claude_settings_with_hook(None, DEFAULT_CLAUDE_HOOK_COMMAND).unwrap();
        let twice = claude_settings_with_hook(Some(&once), DEFAULT_CLAUDE_HOOK_COMMAND).unwrap();
        let v: serde_json::Value = serde_json::from_str(&twice).unwrap();
        // Exactly one group — not duplicated.
        assert_eq!(v["hooks"]["SessionEnd"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn claude_with_hook_preserves_other_content() {
        let existing = r#"{
            "model": "opus",
            "hooks": {
                "PreToolUse": [{"hooks":[{"type":"command","command":"echo hi"}]}],
                "SessionEnd": [{"hooks":[{"type":"command","command":"my-other-hook"}]}]
            }
        }"#;
        let out = claude_settings_with_hook(Some(existing), DEFAULT_CLAUDE_HOOK_COMMAND).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["model"], "opus");
        // PreToolUse untouched.
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "echo hi"
        );
        // The user's other SessionEnd hook survives alongside ours (2 groups).
        let se = v["hooks"]["SessionEnd"].as_array().unwrap();
        assert_eq!(se.len(), 2);
        assert!(claude_hook_present_in(&out));
    }

    #[test]
    fn claude_without_hook_removes_only_ours_and_prunes() {
        let with = claude_settings_with_hook(None, DEFAULT_CLAUDE_HOOK_COMMAND).unwrap();
        let without = claude_settings_without_hook(&with).unwrap();
        assert!(!claude_hook_present_in(&without));
        let v: serde_json::Value = serde_json::from_str(&without).unwrap();
        // The whole empty `hooks` object is pruned.
        assert!(v.get("hooks").is_none());
    }

    #[test]
    fn claude_with_hook_preserves_existing_key_order() {
        // The user's settings.json is hand-maintained; round-tripping it must
        // not silently re-sort their keys. Use a deliberately non-alphabetical
        // order so a sorted (BTreeMap) round-trip is visibly different.
        let existing = r#"{"zebra":1,"model":"opus","alpha":2}"#;
        let out = claude_settings_with_hook(Some(existing), DEFAULT_CLAUDE_HOOK_COMMAND).unwrap();
        let zebra = out.find("zebra").expect("zebra key present");
        let model = out.find("\"model\"").expect("model key present");
        let alpha = out.find("alpha").expect("alpha key present");
        assert!(
            zebra < model && model < alpha,
            "original key order must be preserved, got:\n{out}"
        );
    }

    #[test]
    fn claude_without_hook_keeps_sibling_hooks() {
        let existing = r#"{"hooks":{"SessionEnd":[
            {"hooks":[{"type":"command","command":"my-other-hook"}]},
            {"hooks":[{"type":"command","command":"cinch agent-hook claude-session-end"}]}
        ]}}"#;
        let without = claude_settings_without_hook(existing).unwrap();
        let v: serde_json::Value = serde_json::from_str(&without).unwrap();
        let se = v["hooks"]["SessionEnd"].as_array().unwrap();
        assert_eq!(se.len(), 1);
        assert_eq!(se[0]["hooks"][0]["command"], "my-other-hook");
    }

    #[test]
    fn claude_install_uninstall_roundtrip_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(".claude").join("settings.json");
        assert!(!is_claude_hook_installed(&path));
        install_claude_hook(&path, DEFAULT_CLAUDE_HOOK_COMMAND).unwrap();
        assert!(is_claude_hook_installed(&path));
        // Idempotent second install.
        install_claude_hook(&path, DEFAULT_CLAUDE_HOOK_COMMAND).unwrap();
        assert!(is_claude_hook_installed(&path));
        uninstall_claude_hook(&path).unwrap();
        assert!(!is_claude_hook_installed(&path));
        // Uninstall when absent is a no-op (no error).
        uninstall_claude_hook(&path).unwrap();
    }

    // ── Codex rc editing ───────────────────────────────────────────────────────

    #[test]
    fn codex_block_contains_markers_and_call() {
        let block = codex_wrapper_block(DEFAULT_CINCH_BIN);
        assert!(block.contains(CODEX_BLOCK_START));
        assert!(block.contains(CODEX_BLOCK_END));
        assert!(block.contains("command codex \"$@\""));
        assert!(block.contains("agent-hook codex-exit"));
    }

    #[test]
    fn rc_with_codex_block_is_idempotent_and_preserves_content() {
        let original = "export PATH=/usr/bin\nalias ll='ls -l'\n";
        let once = rc_with_codex_block(original, DEFAULT_CINCH_BIN);
        assert!(once.contains(CODEX_BLOCK_START));
        assert!(once.contains("alias ll='ls -l'"));
        let twice = rc_with_codex_block(&once, DEFAULT_CINCH_BIN);
        // Only one block — not appended twice.
        assert_eq!(twice.matches(CODEX_BLOCK_START).count(), 1);
    }

    #[test]
    fn rc_without_codex_block_removes_block_only() {
        let original = "alias ll='ls -l'\n";
        let with = rc_with_codex_block(original, DEFAULT_CINCH_BIN);
        let without = rc_without_codex_block(&with);
        assert!(!without.contains(CODEX_BLOCK_START));
        assert!(!without.contains(CODEX_BLOCK_END));
        assert!(!without.contains("agent-hook codex-exit"));
        assert!(without.contains("alias ll='ls -l'"));
    }

    #[test]
    fn rc_without_codex_block_absent_is_noop() {
        let original = "alias ll='ls -l'\n";
        assert_eq!(rc_without_codex_block(original), original);
    }

    #[test]
    fn rc_without_codex_block_preserves_tail_when_end_marker_missing() {
        // A dangling START (a torn write, a hand-trim, a partial restore that
        // dropped the END line) must NOT cause everything after it to be
        // silently destroyed — the guard markers exist to tolerate edits, so
        // an unmatched START must fail safe and leave the file unchanged.
        let damaged = format!(
            "export PATH=/usr/bin\n\
             {CODEX_BLOCK_START}\n\
             codex() {{ :; }}\n\
             export EDITOR=vim\n\
             alias gs='git status'\n\
             source ~/.secret_env\n"
        );
        let out = rc_without_codex_block(&damaged);
        for needle in [
            "export PATH=/usr/bin",
            "export EDITOR=vim",
            "alias gs='git status'",
            "source ~/.secret_env",
        ] {
            assert!(
                out.contains(needle),
                "dangling START must not drop {needle:?}; got:\n{out}"
            );
        }
    }

    #[test]
    fn rc_without_codex_block_removes_well_formed_block_among_user_content() {
        // A complete block bracketed by user content on both sides is removed
        // cleanly, leaving everything else intact.
        let rc = format!(
            "before=1\n{}after=2\n",
            codex_wrapper_block(DEFAULT_CINCH_BIN)
        );
        let out = rc_without_codex_block(&rc);
        assert!(!out.contains(CODEX_BLOCK_START));
        assert!(!out.contains(CODEX_BLOCK_END));
        assert!(out.contains("before=1"));
        assert!(out.contains("after=2"));
    }

    #[test]
    fn codex_install_uninstall_roundtrip_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let rc = tmp.path().join(".zshrc");
        std::fs::write(&rc, "alias ll='ls -l'\n").unwrap();
        assert!(!is_codex_wrapper_installed(&rc));
        install_codex_wrapper(&rc, DEFAULT_CINCH_BIN).unwrap();
        assert!(is_codex_wrapper_installed(&rc));
        uninstall_codex_wrapper(&rc).unwrap();
        assert!(!is_codex_wrapper_installed(&rc));
        // Original content survives.
        assert!(std::fs::read_to_string(&rc)
            .unwrap()
            .contains("alias ll='ls -l'"));
    }

    // ── shell detection ────────────────────────────────────────────────────────

    #[test]
    fn shell_kind_classifies_by_basename() {
        assert_eq!(shell_kind(Some("/bin/zsh")), ShellKind::Zsh);
        assert_eq!(shell_kind(Some("/usr/local/bin/bash")), ShellKind::Bash);
        assert_eq!(shell_kind(Some("/opt/homebrew/bin/fish")), ShellKind::Fish);
        assert_eq!(shell_kind(Some("/bin/dash")), ShellKind::Other);
        assert_eq!(shell_kind(None), ShellKind::Other);
    }

    #[test]
    fn codex_target_posix_for_zsh_and_bash() {
        let home = Path::new("/home/u");
        match codex_target(Some("/bin/zsh"), home, "cinch") {
            CodexTarget::Posix(p) => assert_eq!(p, home.join(".zshrc")),
            CodexTarget::Manual(_) => panic!("zsh should auto-edit .zshrc"),
        }
        match codex_target(Some("/bin/bash"), home, "cinch") {
            CodexTarget::Posix(p) => assert_eq!(p, home.join(".bashrc")),
            CodexTarget::Manual(_) => panic!("bash should auto-edit .bashrc"),
        }
    }

    #[test]
    fn codex_target_manual_for_fish_and_unknown() {
        let home = Path::new("/home/u");
        match codex_target(Some("/opt/homebrew/bin/fish"), home, "cinch") {
            CodexTarget::Manual(s) => assert!(s.contains("function codex")),
            CodexTarget::Posix(_) => panic!("fish must be manual"),
        }
        match codex_target(Some("/bin/dash"), home, "cinch") {
            CodexTarget::Manual(s) => assert!(s.contains(CODEX_BLOCK_START)),
            CodexTarget::Posix(_) => panic!("unknown shell must be manual"),
        }
    }

    #[test]
    fn posix_rc_path_maps_known_shells() {
        let home = Path::new("/home/u");
        assert_eq!(
            posix_rc_path(ShellKind::Zsh, home),
            Some(home.join(".zshrc"))
        );
        assert_eq!(
            posix_rc_path(ShellKind::Bash, home),
            Some(home.join(".bashrc"))
        );
        assert_eq!(posix_rc_path(ShellKind::Fish, home), None);
        assert_eq!(posix_rc_path(ShellKind::Other, home), None);
    }
}
