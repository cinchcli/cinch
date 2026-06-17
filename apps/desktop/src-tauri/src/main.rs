// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "builtin-cli")]
use std::path::Path;

#[cfg(feature = "builtin-cli")]
fn is_cli_name(exe: &str) -> bool {
    // The desktop launcher invokes the app as "Cinch" (capital), so matching
    // the lowercase CLI names is unambiguous. `ci` is the short symlink alias.
    matches!(exe, "cinch" | "cinch.exe" | "ci" | "ci.exe")
}

/// Decide whether to run the embedded CLI instead of launching the GUI.
///
/// Normally keyed on argv[0]'s basename (`cinch`/`ci`). But the agent-resume
/// SessionEnd hook / Codex wrapper bakes the app binary's *own* absolute path
/// (basename `Cinch`), so we also dispatch to the CLI when argv[1] is
/// `agent-hook`. A GUI launch never passes `agent-hook`, so this stays
/// unambiguous.
#[cfg(feature = "builtin-cli")]
fn should_dispatch_cli(exe: &str, arg1: Option<&str>) -> bool {
    is_cli_name(exe) || arg1 == Some("agent-hook")
}

#[cfg(feature = "builtin-cli")]
fn invoked_as_cli() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let Some(arg0) = args.first() else {
        return false;
    };
    let exe = Path::new(arg0)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    should_dispatch_cli(exe, args.get(1).map(|s| s.as_str()))
}

#[cfg(all(test, feature = "builtin-cli"))]
mod tests {
    use super::{is_cli_name, should_dispatch_cli};

    #[test]
    fn cli_invocation_names_dispatch_to_cli() {
        assert!(is_cli_name("cinch"));
        assert!(is_cli_name("cinch.exe"));
        assert!(is_cli_name("ci"));
        assert!(is_cli_name("ci.exe"));
    }

    #[test]
    fn desktop_and_unknown_names_do_not_dispatch() {
        assert!(!is_cli_name("Cinch"));
        assert!(!is_cli_name("Cinch.exe"));
        assert!(!is_cli_name("cinchd"));
        assert!(!is_cli_name("cid"));
        assert!(!is_cli_name(""));
    }

    #[test]
    fn agent_hook_subcommand_dispatches_cli_even_when_invoked_as_app() {
        // The baked hook calls the app binary by absolute path (basename
        // "Cinch"); argv[1] == "agent-hook" must still route to the CLI.
        assert!(should_dispatch_cli("Cinch", Some("agent-hook")));
        assert!(should_dispatch_cli("cinch-desktop", Some("agent-hook")));
        // Bare cinch name still dispatches regardless of arg1.
        assert!(should_dispatch_cli("cinch", None));
        // A GUI launch (no agent-hook) must NOT dispatch to the CLI.
        assert!(!should_dispatch_cli("Cinch", None));
        assert!(!should_dispatch_cli("Cinch", Some("push")));
    }
}

fn main() {
    #[cfg(feature = "builtin-cli")]
    if invoked_as_cli() {
        std::process::exit(cinch_cli::run());
    }

    // Default: launch the Tauri desktop app.
    cinch_desktop_lib::run()
}
