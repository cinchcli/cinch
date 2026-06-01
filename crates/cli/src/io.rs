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

/// Write bytes to stdout. A broken pipe (e.g. a downstream `head`) is treated
/// as success; any other write error becomes an `ExitError`.
pub fn write_to_stdout(bytes: &[u8]) -> Result<(), ExitError> {
    match std::io::stdout().write_all(bytes) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(ExitError::new(GENERIC_ERROR, e.to_string(), "")),
    }
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
