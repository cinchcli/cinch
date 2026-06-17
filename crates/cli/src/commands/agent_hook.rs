//! `cinch agent-hook` — wiring for auto-copying an agent's "resume this
//! session" command when a coding session ends. The logic lives in
//! [`client_core::agent_resume`]; this command is the CLI surface.
//!
//! - `claude-session-end` / `codex-exit` are **hidden machine entrypoints**
//!   invoked by the installed Claude `SessionEnd` hook and the Codex shell
//!   wrapper. They are best-effort and silent: a hook must never surface an
//!   error back to the agent.
//! - `enable` / `disable` / `status` install, remove, or report the wiring.
//!   The desktop Settings toggle drives the same `client_core::agent_resume`
//!   functions; this is the CLI-only equivalent.
//!
//! The whole group runs **synchronously** (no tokio, telemetry, or
//! update-check — see `lib.rs`'s early dispatch) so the per-session-end hook
//! stays fast and never trips Claude's hook timeout.

use std::io::Read;

use client_core::agent_resume::{self, Agent, CodexTarget};
use client_core::store::models::{StoredClip, SyncState};
use client_core::store::{self, queries, settings, Store};

use crate::exit::{ExitError, GENERIC_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, clap::Subcommand)]
enum Cmd {
    /// (internal) Claude Code SessionEnd hook entrypoint; reads JSON on stdin.
    #[command(hide = true)]
    ClaudeSessionEnd,
    /// (internal) Codex shell-wrapper exit entrypoint.
    #[command(hide = true)]
    CodexExit {
        /// Only act if a Codex session was written at/after this unix time
        /// (the wrapper's start time), so `codex --version` etc. don't copy.
        #[arg(long)]
        since: Option<i64>,
    },
    /// Turn on resume-on-exit copying for an agent (installs the hook/wrapper).
    Enable {
        #[arg(value_enum)]
        agent: AgentArg,
    },
    /// Turn it off (removes the hook/wrapper).
    Disable {
        #[arg(value_enum)]
        agent: AgentArg,
    },
    /// Show whether resume-on-exit is enabled and installed, per agent.
    Status,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum AgentArg {
    Claude,
    Codex,
}

impl From<AgentArg> for Agent {
    fn from(a: AgentArg) -> Self {
        match a {
            AgentArg::Claude => Agent::Claude,
            AgentArg::Codex => Agent::Codex,
        }
    }
}

/// Sync entrypoint (dispatched from `lib.rs` before the tokio runtime).
pub fn run(args: Args) -> Result<(), ExitError> {
    match args.cmd {
        Cmd::ClaudeSessionEnd => {
            run_claude_session_end();
            Ok(())
        }
        Cmd::CodexExit { since } => {
            run_codex_exit(since);
            Ok(())
        }
        Cmd::Enable { agent } => run_enable(agent.into()),
        Cmd::Disable { agent } => run_disable(agent.into()),
        Cmd::Status => run_status(),
    }
}

// ── Hidden machine entrypoints (best-effort, silent) ──────────────────────────

fn run_claude_session_end() {
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return;
    }
    let Some(parsed) = agent_resume::parse_claude_session_end(&buf) else {
        return;
    };
    if !agent_resume::should_copy_for_reason(&parsed.reason) {
        return;
    }
    copy_resume_clip(Agent::Claude, &parsed.session_id);
}

fn run_codex_exit(since: Option<i64>) {
    let home = agent_resume::codex_home();
    let Some(id) = agent_resume::latest_codex_session_id(&home, since) else {
        return;
    };
    copy_resume_clip(Agent::Codex, &id);
}

/// Best-effort: save a local clip (if the toggle is on) then place it on the
/// clipboard. Swallows every error — a hook must stay silent.
fn copy_resume_clip(agent: Agent, session_id: &str) {
    let Ok(store) = open_store() else {
        return;
    };
    if let Some(cmd) = save_resume_clip(&store, agent, session_id) {
        crate::io::copy_text_to_clipboard(&cmd);
    }
}

/// Save the resume command as a LOCAL clip when the agent's toggle is on.
/// Returns the command string (so the caller can also copy it to the
/// clipboard), or `None` when disabled / on store error.
///
/// Insert-then-copy is deliberate: a running desktop's clipboard poller has a
/// cross-process echo guard (`recent_clip_id_by_content`) that finds this
/// just-saved clip and surfaces it rather than capturing a duplicate. The clip
/// is `SyncState::Local`, so it never reaches the relay.
fn save_resume_clip(store: &Store, agent: Agent, session_id: &str) -> Option<String> {
    if !settings::is_agent_resume_enabled(store, agent.as_str()).unwrap_or(false) {
        return None;
    }
    let cmd = agent_resume::resume_command(agent, session_id);
    let bytes = cmd.clone().into_bytes();
    let content_type = client_core::classify::detect(&bytes);
    let byte_size = bytes.len() as i64;
    let stored = StoredClip {
        id: ulid::Ulid::new().to_string(),
        source: agent.as_str().to_string(),
        content_type: content_type.as_wire().to_string(),
        content: Some(bytes),
        byte_size,
        created_at: chrono::Utc::now().timestamp_millis(),
        sync_state: SyncState::Local,
        ..Default::default()
    };
    queries::insert_clip(store, &stored).ok()?;
    Some(cmd)
}

// ── enable / disable / status ─────────────────────────────────────────────────

/// Where the Codex shell wrapper should be installed for the current shell.
fn codex_install_target() -> CodexTarget {
    let shell = std::env::var("SHELL").ok();
    let home = dirs::home_dir().unwrap_or_default();
    agent_resume::codex_target(shell.as_deref(), &home, agent_resume::DEFAULT_CINCH_BIN)
}

fn run_enable(agent: Agent) -> Result<(), ExitError> {
    let store = open_store()?;
    settings::set_agent_resume_enabled(&store, agent.as_str(), true)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not save setting: {e}"), ""))?;
    match agent {
        Agent::Claude => {
            let path = agent_resume::claude_settings_path().ok_or_else(|| {
                ExitError::new(GENERIC_ERROR, "Could not resolve home directory.", "")
            })?;
            agent_resume::install_claude_hook(&path, agent_resume::DEFAULT_CLAUDE_HOOK_COMMAND)
                .map_err(|e| {
                    ExitError::new(
                        GENERIC_ERROR,
                        format!("Could not install Claude hook: {e}"),
                        "",
                    )
                })?;
            eprintln!("\u{2713} Claude Code will copy its resume command when a session ends.");
            eprintln!("  SessionEnd hook installed in {}", path.display());
        }
        Agent::Codex => match codex_install_target() {
            CodexTarget::Posix(rc) => {
                agent_resume::install_codex_wrapper(&rc, agent_resume::DEFAULT_CINCH_BIN).map_err(
                    |e| {
                        ExitError::new(
                            GENERIC_ERROR,
                            format!("Could not install Codex wrapper: {e}"),
                            "",
                        )
                    },
                )?;
                eprintln!("\u{2713} Codex will copy its resume command when a session ends.");
                eprintln!("  Wrapper added to {}", rc.display());
                eprintln!(
                    "  Restart your terminal (or run `source {}`) to apply.",
                    rc.display()
                );
            }
            CodexTarget::Manual(snippet) => {
                eprintln!(
                    "\u{2713} Enabled. Codex uses a shell function — add this to your shell config:\n"
                );
                println!("{snippet}");
            }
        },
    }
    Ok(())
}

fn run_disable(agent: Agent) -> Result<(), ExitError> {
    let store = open_store()?;
    settings::set_agent_resume_enabled(&store, agent.as_str(), false)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not save setting: {e}"), ""))?;
    match agent {
        Agent::Claude => {
            if let Some(path) = agent_resume::claude_settings_path() {
                agent_resume::uninstall_claude_hook(&path).map_err(|e| {
                    ExitError::new(
                        GENERIC_ERROR,
                        format!("Could not remove Claude hook: {e}"),
                        "",
                    )
                })?;
            }
            eprintln!("\u{2713} Disabled resume-on-exit for Claude Code.");
        }
        Agent::Codex => match codex_install_target() {
            CodexTarget::Posix(rc) => {
                agent_resume::uninstall_codex_wrapper(&rc).map_err(|e| {
                    ExitError::new(
                        GENERIC_ERROR,
                        format!("Could not remove Codex wrapper: {e}"),
                        "",
                    )
                })?;
                eprintln!(
                    "\u{2713} Disabled resume-on-exit for Codex. Restart your terminal to apply."
                );
            }
            CodexTarget::Manual(_) => {
                eprintln!(
                    "\u{2713} Disabled resume-on-exit for Codex. Remove the cinch block from your shell config manually."
                );
            }
        },
    }
    Ok(())
}

fn run_status() -> Result<(), ExitError> {
    let store = open_store()?;
    for agent in [Agent::Claude, Agent::Codex] {
        let enabled = settings::is_agent_resume_enabled(&store, agent.as_str()).unwrap_or(false);
        let installed = match agent {
            Agent::Claude => agent_resume::claude_settings_path()
                .map(|p| agent_resume::is_claude_hook_installed(&p))
                .unwrap_or(false),
            Agent::Codex => match codex_install_target() {
                CodexTarget::Posix(rc) => agent_resume::is_codex_wrapper_installed(&rc),
                CodexTarget::Manual(_) => false,
            },
        };
        println!(
            "{:<7} enabled={enabled} installed={installed}",
            agent.as_str()
        );
    }
    Ok(())
}

fn open_store() -> Result<Store, ExitError> {
    let path = store::default_db_path().map_err(|e| {
        ExitError::new(
            GENERIC_ERROR,
            format!("Could not find local store: {e}"),
            "",
        )
    })?;
    Store::open(&path).map_err(|e| {
        ExitError::new(
            GENERIC_ERROR,
            format!("Could not open local store: {e}"),
            "",
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_store() -> Store {
        Store::open(std::path::Path::new(":memory:")).unwrap()
    }

    #[test]
    fn save_resume_clip_inserts_local_when_enabled() {
        let store = mem_store();
        settings::set_agent_resume_enabled(&store, "claude", true).unwrap();
        let cmd = save_resume_clip(&store, Agent::Claude, "sid-1").unwrap();
        assert_eq!(cmd, "claude --resume sid-1");
        // The clip is queryable by content — which is exactly how the desktop
        // echo guard finds it — proving it landed in the local store.
        let found = queries::recent_clip_id_by_content(&store, cmd.as_bytes(), 0).unwrap();
        assert!(found.is_some(), "saved clip must be in the local store");
    }

    #[test]
    fn save_resume_clip_noops_when_disabled() {
        let store = mem_store();
        // Default is disabled.
        assert!(save_resume_clip(&store, Agent::Codex, "sid-2").is_none());
        let found = queries::recent_clip_id_by_content(&store, b"codex resume sid-2", 0).unwrap();
        assert!(found.is_none(), "nothing should be saved when disabled");
    }

    #[test]
    fn agent_arg_maps_to_core_agent() {
        assert_eq!(Agent::from(AgentArg::Claude), Agent::Claude);
        assert_eq!(Agent::from(AgentArg::Codex), Agent::Codex);
    }
}
