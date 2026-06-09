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
    is_cli_name(exe)
}

#[cfg(all(test, feature = "builtin-cli"))]
mod tests {
    use super::is_cli_name;

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
}

fn main() {
    #[cfg(feature = "builtin-cli")]
    if invoked_as_cli() {
        std::process::exit(cinch_cli::run());
    }

    // Default: launch the Tauri desktop app.
    cinch_desktop_lib::run()
}
