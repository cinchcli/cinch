//! Shared stdout / clipboard output helpers for CLI commands.
//!
//! Consolidates the previously-duplicated `write_to_stdout` (three identical
//! copies in `get`/`transform`/`ai`) and `copy_text_to_clipboard` (four copies
//! with an *inconsistent* error policy: `get`/`pull` correctly warned and
//! continued, while `transform`/`ai` aborted the whole command on a clipboard
//! failure and silently dropped the user's result). The single
//! `copy_text_to_clipboard` here is always best-effort, so the primary output
//! can never be lost just because the clipboard was unavailable.

use crate::exit::{ExitError, GENERIC_ERROR};
use std::io::Write;
use std::path::Path;
use std::process::ExitStatus;

/// Write bytes to stdout. A broken pipe (e.g. a downstream `head`) is treated
/// as success; any other write error becomes an `ExitError`.
pub fn write_to_stdout(bytes: &[u8]) -> Result<(), ExitError> {
    match std::io::stdout().write_all(bytes) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(ExitError::new(GENERIC_ERROR, e.to_string(), "")),
    }
}

/// Choose the editor command: `$VISUAL`, then `$EDITOR`, then `vi`.
/// Split out from env lookup so it is unit-testable.
pub(crate) fn pick_editor_from(visual: Option<String>, editor: Option<String>) -> String {
    visual
        .filter(|s| !s.trim().is_empty())
        .or_else(|| editor.filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "vi".to_string())
}

pub(crate) fn pick_editor() -> String {
    pick_editor_from(std::env::var("VISUAL").ok(), std::env::var("EDITOR").ok())
}

/// Open `path` in the user's editor and wait for it to exit. The editor string
/// may contain arguments (e.g. `code --wait`); we pass `path` as a positional
/// arg via the shell's `"$@"` so quoting/spaces are handled correctly.
#[cfg(not(windows))]
pub(crate) fn spawn_editor(editor: &str, path: &Path) -> std::io::Result<ExitStatus> {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$@\""))
        .arg("sh") // $0
        .arg(path) // becomes $1, expanded by "$@"
        .status()
}

#[cfg(windows)]
pub(crate) fn spawn_editor(editor: &str, path: &Path) -> std::io::Result<ExitStatus> {
    std::process::Command::new("cmd")
        .arg("/C")
        .arg(editor)
        .arg(path)
        .status()
}

/// Best-effort copy of `text` to the system clipboard. Returns `true` on
/// success. On failure it prints a warning to stderr and returns `false` — it
/// NEVER aborts the command, so the caller can fall back to stdout and the
/// user's result is never lost.
pub fn copy_text_to_clipboard(text: &str) -> bool {
    use arboard::Clipboard;
    match Clipboard::new() {
        Ok(mut cb) => match cb.set_text(text) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("Warning: clipboard write failed: {e}");
                false
            }
        },
        Err(_) => {
            eprintln!("Warning: could not open system clipboard");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pick_editor_prefers_visual_then_editor_then_vi() {
        use super::pick_editor_from;
        assert_eq!(
            pick_editor_from(Some("nvim".into()), Some("vim".into())),
            "nvim"
        );
        assert_eq!(pick_editor_from(None, Some("vim".into())), "vim");
        assert_eq!(
            pick_editor_from(Some("  ".into()), Some("vim".into())),
            "vim"
        );
        assert_eq!(pick_editor_from(None, None), "vi");
    }
}
