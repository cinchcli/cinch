// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(feature = "builtin-cli")]
use std::path::Path;

#[cfg(feature = "builtin-cli")]
fn invoked_as_cli() -> bool {
    let args: Vec<String> = std::env::args().collect();
    if args.is_empty() {
        return false;
    }
    let exe = Path::new(&args[0])
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    // If the binary is symlinked/renamed as "cinch", dispatch to CLI.
    // The desktop launcher invokes as "Cinch" (capital), so this is unambiguous.
    matches!(exe, "cinch" | "cinch.exe")
}

fn main() {
    #[cfg(feature = "builtin-cli")]
    if invoked_as_cli() {
        std::process::exit(cinch_cli::run());
    }

    // Default: launch the Tauri desktop app.
    cinch_desktop_lib::run()
}
