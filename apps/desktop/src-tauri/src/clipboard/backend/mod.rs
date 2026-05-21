//! Per-platform clipboard backends. Internal to the clipboard module —
//! external callers should go through `ClipboardService`.
//!
//! Scope: the desktop app is macOS-only for the Launch-Ready Polish milestone
//! (see `.planning/PROJECT.md` § Constraints and phase 01 D-02/D-10). The
//! `Backend` trait is deliberately preserved so `ScriptedBackend` in
//! `clipboard::service::tests` can substitute a scripted implementation for
//! unit tests.

pub mod types;

pub use types::{ClipboardError, PollContent, PollSnapshot};

pub(crate) mod macos;

/// Platform-agnostic contract every backend implements.
pub(crate) trait Backend: Send {
    fn read_snapshot(&mut self) -> Result<PollSnapshot, ClipboardError>;
    fn write_text(&mut self, content: &str) -> Result<(), ClipboardError>;
    /// Write PNG-encoded image bytes to the clipboard. Backends that need raw
    /// RGBA (arboard) decode internally.
    fn write_image_png(&mut self, png_bytes: &[u8]) -> Result<(), ClipboardError>;
    /// Platform default list of excluded app identifiers, in the format the
    /// backend's `PollSnapshot::app_identity` produces.
    fn default_excluded_apps(&self) -> Vec<String>;
}

/// Construct the platform-appropriate backend for this build target.
pub(crate) fn platform_default() -> Box<dyn Backend + Send> {
    Box::new(macos::MacBackend::new())
}
