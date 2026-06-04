//! Source-agnostic reader for agent coding-session transcripts.
//!
//! An agent session (Claude Code today; codex / gemini-cli later) is a noisy
//! JSONL transcript mixing user/assistant text with tool calls, tool results,
//! thinking blocks, and base64 attachments. This module parses such a
//! transcript into a clean, ordered list of [`Answer`]s and renders a
//! selection to Markdown.
//!
//! Layout:
//! - [`model`] — plain, source-agnostic data types ([`Session`], [`Answer`],
//!   [`AnswerPart`], …). These are *not* the raw JSONL wire shapes.
//! - [`source`] — the [`SessionSource`] trait plus the [`ClaudeSource`] impl,
//!   which owns the `cwd → ~/.claude/projects/<encoded>` mapping, session
//!   discovery, JSONL parsing, and grouping records into answers.
//! - [`render`] — [`markdown`] turns selected answers into clean Markdown per
//!   [`RenderOpts`].
//!
//! Errors follow the crate convention (mirror [`crate::store::StoreError`]).

pub mod model;
pub mod render;
pub mod source;

pub use model::{Answer, AnswerPart, Prompt, Session, SessionRef};
pub use render::{answer_is_empty, markdown, RenderOpts};
pub use source::{ClaudeSource, SessionSource};

/// Errors surfaced while listing, loading, or parsing a session.
///
/// Per-line JSONL parse errors do **not** flow through `Json` — the parser in
/// [`source`] is tolerant and skips malformed lines. `Json` is reserved for
/// whole-record decode failures a caller chooses to surface; in practice the
/// loader stays tolerant, so `Json` may only appear in tests. It is kept as a
/// typed escape hatch for completeness.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no sessions found for {0}")]
    NoSessions(String),
    #[error("session not found: {0}")]
    NotFound(String),
    #[error("home directory unavailable")]
    NoHome,
}
