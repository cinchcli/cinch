//! Shared types exchanged between `ClipboardService` and platform backends.

use std::fmt;

/// A single observation of the clipboard state.
#[derive(Debug, Clone)]
pub struct PollSnapshot {
    /// Opaque monotonic change token. Real on macOS (NSPasteboard.changeCount).
    /// `None` when the backend cannot report one, in which case the service
    /// falls back to full snapshot comparison.
    pub token: Option<u64>,
    pub content: PollContent,
    /// Frontmost application identifier. Bundle id on macOS. `None` when the
    /// backend cannot determine it.
    pub app_identity: Option<String>,
}

// Variants are constructed only inside `backend::macos`; on Linux/Windows the
// enum is referenced solely by match arms and type signatures, so suppress the
// dead-code lint there (cinch-desktop's runtime is macOS-only — see
// `backend::platform_default`).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone)]
pub enum PollContent {
    Text(String),
    /// PNG-encoded image bytes. Backends that receive raw RGBA (arboard)
    /// encode to PNG internally after a size gate on `width * height * 4`.
    ImagePng(Vec<u8>),
    Empty,
    /// Clipboard has content we intentionally don't capture: RTF, file-uri-list,
    /// rich formatted data, etc. Also used for NSPasteboard concealed/transient
    /// items (plan 01-04 — `MacBackend::read_snapshot` emits this when it sees
    /// `org.nspasteboard.ConcealedType` or `org.nspasteboard.TransientType`).
    Unsupported,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Debug, Clone)]
pub enum ClipboardError {
    /// Content exceeded the maximum allowed size. Preserved as an expansion
    /// point for future monitor/backend refactors (e.g. `monitor.rs` returning
    /// this instead of logging oversized images inline). Not currently
    /// constructed anywhere in the macOS-only code path.
    #[allow(dead_code)]
    Oversized {
        bytes: usize,
        limit: usize,
    },
    Backend(String),
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClipboardError::Oversized { bytes, limit } => {
                write!(
                    f,
                    "clipboard content too large: {} bytes (limit {})",
                    bytes, limit
                )
            }
            ClipboardError::Backend(s) => write!(f, "backend error: {}", s),
        }
    }
}

impl std::error::Error for ClipboardError {}
