//! Plain, source-agnostic data types for a parsed session and its answers.
//!
//! These are *domain* types, not the raw JSONL wire shapes. Raw transcript
//! records are parsed via `serde_json::Value` in `source.rs`; they are never
//! deserialized straight into these structs.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// One-line preview / title cap (in `char`s, not bytes).
const PREVIEW_CAP: usize = 60;

/// Lightweight reference to a session file, used for listing/picking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRef {
    /// Session uuid (the file stem).
    pub id: String,
    /// Derived title: ai-title or first-prompt fallback.
    pub title: Option<String>,
    /// Absolute path to the `*.jsonl` transcript.
    pub path: PathBuf,
    /// File mtime in unix milliseconds, for sort/display.
    pub mtime_ms: i64,
}

/// A fully parsed session: ordered answers plus metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub title: Option<String>,
    pub path: PathBuf,
    pub answers: Vec<Answer>,
}

/// The eliciting user prompt for an answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    pub text: String,
}

/// One complete assistant turn: a real user prompt plus all interleaved
/// assistant/tool steps up to (not including) the next real user prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    /// 0-based order within the session.
    pub index: usize,
    /// The user prompt that started this answer (absent only for malformed
    /// transcripts).
    pub prompt: Option<Prompt>,
    /// Rendered content, in source order.
    pub parts: Vec<AnswerPart>,
}

/// One piece of an answer's content, in source order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnswerPart {
    /// Assistant prose.
    Text(String),
    /// A tool invocation. `input` is a compact JSON string.
    ToolUse { name: String, input: String },
    /// A tool result. The full (already-untruncated) text; render truncates.
    ToolResult { truncated_text: String },
    /// An assistant thinking block (often empty; render gates on a flag).
    Thinking(String),
    /// A short placeholder label (<= 40 chars) standing in for stripped
    /// base64 / data-URI / file-snapshot noise.
    Attachment { label: String },
}

impl Answer {
    /// One-line preview for the picker.
    ///
    /// Prefers the prompt text, else the first `Text` part, else `"(no text)"`.
    /// The result is single-lined (newlines collapsed to spaces) and truncated
    /// with an ellipsis when over [`PREVIEW_CAP`] chars.
    pub fn preview(&self) -> String {
        let raw = self
            .prompt
            .as_ref()
            .map(|p| p.text.as_str())
            .filter(|t| !t.trim().is_empty())
            .or_else(|| {
                self.parts.iter().find_map(|part| match part {
                    AnswerPart::Text(t) if !t.trim().is_empty() => Some(t.as_str()),
                    _ => None,
                })
            })
            .unwrap_or("(no text)");
        truncate_one_line(raw, PREVIEW_CAP)
    }

    /// Derive a short title from the prompt (first line, ~60 chars) for use as
    /// a label fallback.
    pub fn title(&self) -> Option<String> {
        let text = self.prompt.as_ref()?.text.as_str();
        let first_line = text.lines().next().unwrap_or("").trim();
        if first_line.is_empty() {
            return None;
        }
        Some(truncate_chars(first_line, PREVIEW_CAP))
    }
}

/// Collapse all whitespace runs (including newlines) to single spaces, trim,
/// then truncate to `cap` chars with a trailing ellipsis.
fn truncate_one_line(s: &str, cap: usize) -> String {
    let single: String = {
        let mut out = String::with_capacity(s.len());
        let mut prev_space = false;
        for ch in s.chars() {
            if ch.is_whitespace() {
                if !prev_space {
                    out.push(' ');
                    prev_space = true;
                }
            } else {
                out.push(ch);
                prev_space = false;
            }
        }
        out.trim().to_string()
    };
    truncate_chars(&single, cap)
}

/// Truncate to `cap` chars (UTF-8 safe), appending `…` when shortened.
fn truncate_chars(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        return s.to_string();
    }
    let mut out: String = s.chars().take(cap).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer_with_all_parts(prompt: Option<&str>) -> Answer {
        Answer {
            index: 0,
            prompt: prompt.map(|t| Prompt {
                text: t.to_string(),
            }),
            parts: vec![
                AnswerPart::Text("hello world".into()),
                AnswerPart::ToolUse {
                    name: "Bash".into(),
                    input: "{\"command\":\"ls\"}".into(),
                },
                AnswerPart::ToolResult {
                    truncated_text: "file-a\nfile-b".into(),
                },
                AnswerPart::Thinking("reasoning".into()),
                AnswerPart::Attachment {
                    label: "image.png".into(),
                },
            ],
        }
    }

    #[test]
    fn preview_single_lines_and_truncates() {
        let long = "first line\nsecond   line\twith\ttabs and a very long tail that keeps going well past the cap boundary";
        let a = answer_with_all_parts(Some(long));
        let p = a.preview();
        assert!(!p.contains('\n'));
        assert!(!p.contains('\t'));
        assert!(!p.contains("  "), "whitespace runs collapsed: {p:?}");
        assert!(p.ends_with('…'), "long preview is truncated: {p:?}");
        assert_eq!(p.chars().count(), PREVIEW_CAP + 1); // cap + ellipsis
    }

    #[test]
    fn preview_falls_back_to_first_text_part_then_placeholder() {
        let a = answer_with_all_parts(None);
        assert_eq!(a.preview(), "hello world");

        let empty = Answer {
            index: 0,
            prompt: None,
            parts: vec![AnswerPart::Attachment { label: "x".into() }],
        };
        assert_eq!(empty.preview(), "(no text)");
    }

    #[test]
    fn title_returns_first_line_trimmed() {
        let a = answer_with_all_parts(Some("  Fix the parser bug  \nmore detail here"));
        assert_eq!(a.title().as_deref(), Some("Fix the parser bug"));
    }

    #[test]
    fn title_none_without_prompt() {
        let a = answer_with_all_parts(None);
        assert_eq!(a.title(), None);
    }
}
