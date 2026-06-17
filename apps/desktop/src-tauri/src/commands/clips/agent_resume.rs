//! Desktop Settings commands for "copy the agent's resume command on exit".
//!
//! Thin glue over [`client_core::agent_resume`] (which owns all the logic and
//! is unit-tested there). Toggling an agent both persists the per-agent
//! setting and installs/removes the wiring: a `SessionEnd` hook in
//! `~/.claude/settings.json` for Claude Code, or a guarded `codex()` function
//! in the shell rc for Codex (Codex has no native session-end event).

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::State;

use crate::SharedStore;
use client_core::agent_resume::{self, Agent, CodexTarget};
use client_core::store::settings;

/// Per-agent state for the resume-on-exit feature, surfaced to Settings.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AgentResumeConfig {
    pub claude_enabled: bool,
    pub codex_enabled: bool,
    /// Whether the SessionEnd hook / shell wrapper is actually present on disk,
    /// so the UI can flag drift if the user removed it by hand.
    pub claude_installed: bool,
    pub codex_installed: bool,
    /// True when Codex can't be auto-installed for this shell (fish / unknown):
    /// the user pastes a snippet by hand, so `codex_installed` is never known
    /// and the UI must not treat "enabled but not installed" as drift here.
    pub codex_manual_shell: bool,
}

/// Outcome of a toggle, so the UI can tell the user what changed.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AgentResumeResult {
    /// True after a Codex change — the shell wrapper only loads in a new shell.
    pub needs_shell_restart: bool,
    /// Absolute paths cinch edited (the rc file or settings.json).
    pub files_modified: Vec<String>,
    /// For fish / unknown shells: the snippet to paste manually (no auto-edit).
    pub manual_snippet: Option<String>,
}

/// Resolve the Codex install target for the desktop's environment. `bin` is the
/// token embedded in the wrapper — an absolute path when enabling, or the bare
/// default when we only need the rc path (config read / disable).
fn codex_target_for(bin: &str) -> CodexTarget {
    let shell = std::env::var("SHELL").ok();
    let home = dirs::home_dir().unwrap_or_default();
    agent_resume::codex_target(shell.as_deref(), &home, bin)
}

/// Absolute path of the running binary, baked into the installed hook / wrapper
/// so they invoke this exact app regardless of PATH ordering or a stale `cinch`.
/// The app's argv dispatch routes the `agent-hook` subcommand to the CLI even
/// though the bundle's binary basename is `Cinch`.
fn current_exe() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()
}

#[tauri::command]
#[specta::specta]
pub fn get_agent_resume_config(store: State<'_, SharedStore>) -> Result<AgentResumeConfig, String> {
    let claude_enabled = settings::is_agent_resume_enabled(&store, Agent::Claude.as_str())
        .map_err(|e| e.to_string())?;
    let codex_enabled = settings::is_agent_resume_enabled(&store, Agent::Codex.as_str())
        .map_err(|e| e.to_string())?;
    let claude_installed = agent_resume::claude_settings_path()
        .map(|p| agent_resume::is_claude_hook_installed(&p))
        .unwrap_or(false);
    let (codex_installed, codex_manual_shell) =
        match codex_target_for(agent_resume::DEFAULT_CINCH_BIN) {
            CodexTarget::Posix(rc) => (agent_resume::is_codex_wrapper_installed(&rc), false),
            CodexTarget::Manual(_) => (false, true),
        };
    Ok(AgentResumeConfig {
        claude_enabled,
        codex_enabled,
        claude_installed,
        codex_installed,
        codex_manual_shell,
    })
}

#[tauri::command]
#[specta::specta]
pub fn set_agent_resume_enabled(
    store: State<'_, SharedStore>,
    agent: Agent,
    enabled: bool,
) -> Result<AgentResumeResult, String> {
    settings::set_agent_resume_enabled(&store, agent.as_str(), enabled)
        .map_err(|e| e.to_string())?;
    match agent {
        Agent::Claude => set_claude(enabled),
        Agent::Codex => set_codex(enabled),
    }
}

fn set_claude(enabled: bool) -> Result<AgentResumeResult, String> {
    let path = agent_resume::claude_settings_path()
        .ok_or_else(|| "Could not resolve home directory".to_string())?;
    if enabled {
        let command = agent_resume::claude_hook_command(current_exe().as_deref());
        agent_resume::install_claude_hook(&path, &command).map_err(|e| e.to_string())?;
    } else {
        agent_resume::uninstall_claude_hook(&path).map_err(|e| e.to_string())?;
    }
    Ok(AgentResumeResult {
        needs_shell_restart: false,
        files_modified: vec![path.display().to_string()],
        manual_snippet: None,
    })
}

fn set_codex(enabled: bool) -> Result<AgentResumeResult, String> {
    let bin = agent_resume::codex_bin_token(current_exe().as_deref());
    match codex_target_for(&bin) {
        CodexTarget::Posix(rc) => {
            if enabled {
                agent_resume::install_codex_wrapper(&rc, &bin).map_err(|e| e.to_string())?;
            } else {
                agent_resume::uninstall_codex_wrapper(&rc).map_err(|e| e.to_string())?;
            }
            Ok(AgentResumeResult {
                needs_shell_restart: true,
                files_modified: vec![rc.display().to_string()],
                manual_snippet: None,
            })
        }
        // fish / unknown shell: nothing auto-edited; hand back the snippet to
        // paste (only when enabling).
        CodexTarget::Manual(snippet) => Ok(AgentResumeResult {
            needs_shell_restart: true,
            files_modified: vec![],
            manual_snippet: enabled.then_some(snippet),
        }),
    }
}
