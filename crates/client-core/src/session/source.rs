//! `SessionSource` trait plus the `ClaudeSource` implementation.
//!
//! Owns the `cwd → ~/.claude/projects/<encoded>` mapping, session discovery
//! and listing, tolerant JSONL parsing, and grouping records into `Answer`s.
//!
//! Parsing is intentionally synchronous: it is pure filesystem I/O, matching
//! the `store` / `transform` conventions. `async-trait` in this crate is
//! reserved for the network transport.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::model::{Answer, AnswerPart, Prompt, Session, SessionRef};
use super::SessionError;

/// Selects which session to load, relative to a working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSelector {
    /// Default: the most recently modified `*.jsonl`.
    Latest,
    /// Positional `SESSION` arg: matches a file-stem prefix.
    IdPrefix(String),
    /// An already-resolved path (used by `--pick`).
    Path(PathBuf),
}

/// A reader for one agent tool's session transcripts.
pub trait SessionSource {
    /// List sessions for the project rooted at `cwd`, newest first by mtime.
    fn list_sessions(&self, cwd: &Path) -> Result<Vec<SessionRef>, SessionError>;
    /// Load + fully parse a session selected by `selector` relative to `cwd`.
    fn load(&self, cwd: &Path, selector: &SessionSelector) -> Result<Session, SessionError>;
}

/// Reads Claude Code transcripts from `~/.claude/projects/<encoded-cwd>/`.
pub struct ClaudeSource;

impl ClaudeSource {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ClaudeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionSource for ClaudeSource {
    fn list_sessions(&self, cwd: &Path) -> Result<Vec<SessionRef>, SessionError> {
        let dir = projects_dir(cwd)?;
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => return Err(SessionError::NoSessions(cwd.display().to_string())),
        };

        let mut refs: Vec<SessionRef> = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let id = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let title = lightweight_title(&path);
            refs.push(SessionRef {
                id,
                title,
                mtime_ms: mtime_ms(&path),
                path,
            });
        }

        if refs.is_empty() {
            return Err(SessionError::NoSessions(cwd.display().to_string()));
        }
        refs.sort_by_key(|r| std::cmp::Reverse(r.mtime_ms));
        Ok(refs)
    }

    fn load(&self, cwd: &Path, selector: &SessionSelector) -> Result<Session, SessionError> {
        let path = match selector {
            SessionSelector::Path(p) => p.clone(),
            SessionSelector::Latest => {
                self.list_sessions(cwd)?
                    .into_iter()
                    .next()
                    .ok_or_else(|| SessionError::NoSessions(cwd.display().to_string()))?
                    .path
            }
            SessionSelector::IdPrefix(pre) => {
                self.list_sessions(cwd)?
                    .into_iter()
                    .find(|r| r.id.starts_with(pre))
                    .ok_or_else(|| SessionError::NotFound(pre.clone()))?
                    .path
            }
        };
        parse_session(&path)
    }
}

// --- path encoding -------------------------------------------------------

/// Claude project-dir encoding: replace every `/` in the path with `-`.
///
/// Absolute macOS paths start with `/`, so the result already begins with a
/// leading `-` (e.g. `/Users/x` → `-Users-x`). No second leading dash is
/// added.
pub(crate) fn encode_cwd(cwd: &Path) -> String {
    cwd.to_string_lossy().replace('/', "-")
}

fn projects_dir(cwd: &Path) -> Result<PathBuf, SessionError> {
    Ok(dirs::home_dir()
        .ok_or(SessionError::NoHome)?
        .join(".claude/projects")
        .join(encode_cwd(cwd)))
}

fn mtime_ms(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Cheap title scan for listing: parse the file and derive its title.
///
/// Acceptable for MVP session counts. If perf matters later, scan only the
/// first + last N lines; kept simple for now.
fn lightweight_title(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let records = parse_records(&text);
    derive_title(&records)
}

// --- parsing -------------------------------------------------------------

/// Parse a transcript file into a fully grouped [`Session`].
fn parse_session(path: &Path) -> Result<Session, SessionError> {
    let text = std::fs::read_to_string(path)?;
    let records = parse_records(&text);
    let title = derive_title(&records);
    let answers = group_answers(&records);
    let id = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Session {
        id,
        title,
        path: path.to_path_buf(),
        answers,
    })
}

/// Parse each non-empty line as JSON, tolerating (skipping) malformed lines.
fn parse_records(text: &str) -> Vec<Value> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect()
}

// --- record classification ----------------------------------------------

pub(crate) fn record_type(r: &Value) -> Option<&str> {
    r.get("type").and_then(Value::as_str)
}

/// Claude injects synthetic turns into the transcript: skill-text injections,
/// `<local-command-caveat>` blocks, and system reminders are flagged `isMeta`,
/// while sub-agent (Task) turns are flagged `isSidechain`. Neither belongs to
/// the real top-level conversation, so both `user` and `assistant` such records
/// must be ignored when grouping answers and deriving a title.
pub(crate) fn is_injected_meta(r: &Value) -> bool {
    r.get("isMeta").and_then(Value::as_bool) == Some(true)
        || r.get("isSidechain").and_then(Value::as_bool) == Some(true)
}

/// A `user` record whose content is a real prompt: a string, or an array whose
/// first element is not a `tool_result`. Only this kind starts a new answer.
/// Injected meta/sidechain turns never count, even when shaped like a prompt.
pub(crate) fn is_real_user_prompt(r: &Value) -> bool {
    if record_type(r) != Some("user") || is_injected_meta(r) {
        return false;
    }
    let content = match r.get("message").and_then(|m| m.get("content")) {
        Some(c) => c,
        None => return false,
    };
    if content.is_string() {
        return true;
    }
    if let Some(arr) = content.as_array() {
        // Empty array → not a usable prompt.
        let first = match arr.first() {
            Some(v) => v,
            None => return false,
        };
        let first_type = first.get("type").and_then(Value::as_str);
        return first_type != Some("tool_result");
    }
    false
}

/// A `user` record carrying only tool results (array of `tool_result` blocks).
/// Continues the current answer, never starts a new one.
pub(crate) fn is_tool_result_user(r: &Value) -> bool {
    if record_type(r) != Some("user") {
        return false;
    }
    let arr = match r
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
    {
        Some(a) if !a.is_empty() => a,
        _ => return false,
    };
    arr.iter()
        .all(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
}

/// Extract a user prompt's text: the string content, or the concatenation of
/// `text` blocks in an array content.
pub(crate) fn user_prompt_text(r: &Value) -> Option<String> {
    let content = r.get("message").and_then(|m| m.get("content"))?;
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    if let Some(arr) = content.as_array() {
        let mut buf = String::new();
        for block in arr {
            if block.get("type").and_then(Value::as_str) == Some("text") {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    if !buf.is_empty() {
                        buf.push('\n');
                    }
                    buf.push_str(t);
                }
            }
        }
        if !buf.is_empty() {
            return Some(buf);
        }
    }
    None
}

// --- answer grouping -----------------------------------------------------

pub(crate) fn group_answers(records: &[Value]) -> Vec<Answer> {
    let mut answers: Vec<Answer> = Vec::new();
    let mut current: Option<Answer> = None;
    let mut index = 0;

    for r in records {
        // Skip injected meta/sidechain turns entirely so they neither start a
        // phantom answer nor pollute the current one (a sub-agent `assistant`
        // record would otherwise append to the active answer).
        if is_injected_meta(r) {
            continue;
        }
        match record_type(r) {
            Some("user") if is_real_user_prompt(r) => {
                if let Some(a) = current.take() {
                    answers.push(a);
                }
                current = Some(Answer {
                    index,
                    prompt: user_prompt_text(r).map(|text| Prompt { text }),
                    parts: Vec::new(),
                });
                index += 1;
            }
            Some("user") if is_tool_result_user(r) => {
                if let Some(a) = current.as_mut() {
                    push_tool_results(a, r);
                }
            }
            Some("assistant") => {
                if let Some(a) = current.as_mut() {
                    push_assistant_parts(a, r);
                }
            }
            // attachment / mode / permission-mode / last-prompt / ai-title /
            // file-history-snapshot / summary / task_reminder / unknown → skip.
            _ => {}
        }
    }

    if let Some(a) = current.take() {
        answers.push(a);
    }
    answers
}

fn push_assistant_parts(a: &mut Answer, r: &Value) {
    let blocks = match r
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
    {
        Some(b) => b,
        None => return,
    };
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    if !t.trim().is_empty() {
                        a.parts.push(AnswerPart::Text(t.to_string()));
                    }
                }
            }
            Some("thinking") => {
                // The `thinking` string is often empty; still push so render
                // can decide (it gates on `include_thinking`).
                let t = block
                    .get("thinking")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                a.parts.push(AnswerPart::Thinking(t));
            }
            Some("tool_use") => {
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let input = compact_json(block.get("input").unwrap_or(&Value::Null));
                a.parts.push(AnswerPart::ToolUse { name, input });
            }
            _ => {}
        }
    }
}

fn push_tool_results(a: &mut Answer, r: &Value) {
    let blocks = match r
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
    {
        Some(b) => b,
        None => return,
    };
    for block in blocks {
        if block.get("type").and_then(Value::as_str) != Some("tool_result") {
            continue;
        }
        let text = block.get("content").map(value_to_text).unwrap_or_default();
        if looks_like_attachment(&text) {
            a.parts.push(AnswerPart::Attachment {
                label: attachment_label(&text),
            });
        } else {
            a.parts.push(AnswerPart::ToolResult {
                truncated_text: text,
            });
        }
    }
}

// --- title derivation ----------------------------------------------------

pub(crate) fn derive_title(records: &[Value]) -> Option<String> {
    // 1. An explicit ai-title record wins.
    for r in records {
        if record_type(r) == Some("ai-title") {
            if let Some(t) = r.get("aiTitle").and_then(Value::as_str) {
                let trimmed = t.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
        }
    }
    // 2. The first real user prompt's first line, capped.
    for r in records {
        if is_real_user_prompt(r) {
            if let Some(text) = user_prompt_text(r) {
                let first_line = text.lines().next().unwrap_or("").trim();
                if !first_line.is_empty() {
                    return Some(truncate_title(first_line, 60));
                }
            }
        }
    }
    // 3. Caller falls back to the session id.
    None
}

fn truncate_title(s: &str, cap: usize) -> String {
    if s.chars().count() <= cap {
        return s.to_string();
    }
    let mut out: String = s.chars().take(cap).collect();
    out.push('…');
    out
}

// --- attachment / placeholder helpers ------------------------------------

/// Conservative sniff for base64 / data-URI / file-snapshot blobs that should
/// be replaced with a placeholder rather than rendered verbatim.
pub(crate) fn looks_like_attachment(s: &str) -> bool {
    if s.contains("data:") && s.contains(";base64,") {
        return true;
    }
    if s.contains("file-history-snapshot") || s.contains("<file-snapshot") {
        return true;
    }
    // A long run that is overwhelmingly base64-charset (kept conservative so
    // real tool output is not hidden).
    if s.len() > 256 {
        let total = s.chars().count();
        let base64ish = s
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '\n' | '\r'))
            .count();
        if total > 0 && (base64ish as f64 / total as f64) > 0.97 {
            return true;
        }
    }
    false
}

/// Derive a short (<= 40 char) placeholder label for an attachment blob.
pub(crate) fn attachment_label(s: &str) -> String {
    let label = if let Some(rest) = s.split("data:").nth(1) {
        // `image/png;base64,...` → `image.png`
        let mime = rest.split(';').next().unwrap_or("");
        match mime {
            "image/png" => "image.png".to_string(),
            "image/jpeg" | "image/jpg" => "image.jpg".to_string(),
            "image/gif" => "image.gif".to_string(),
            "image/webp" => "image.webp".to_string(),
            m if m.starts_with("image/") => {
                let ext = m.trim_start_matches("image/");
                format!("image.{ext}")
            }
            "" => "attachment".to_string(),
            other => other.replace('/', "."),
        }
    } else if s.contains("file-history-snapshot") || s.contains("<file-snapshot") {
        "file-snapshot".to_string()
    } else {
        "attachment".to_string()
    };
    if label.chars().count() > 40 {
        label.chars().take(40).collect()
    } else {
        label
    }
}

/// Render a `tool_result` `content` value to text: a string as-is, an array of
/// content blocks joined by their `text` fields, otherwise compact JSON.
pub(crate) fn value_to_text(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(arr) = v.as_array() {
        let mut buf = String::new();
        for block in arr {
            if let Some(t) = block.get("text").and_then(Value::as_str) {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(t);
            }
        }
        if !buf.is_empty() {
            return buf;
        }
    }
    compact_json(v)
}

fn compact_json(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Small, hand-written synthetic transcript — never real session data.
    // One JSON record per line.
    const FIXTURE: &str = concat!(
        r#"{"type":"mode","mode":"normal","sessionId":"s1"}"#,
        "\n",
        r#"{"type":"ai-title","aiTitle":"Fix the parser","sessionId":"s1"}"#,
        "\n",
        r#"{"type":"permission-mode","permissionMode":"bypassPermissions","sessionId":"s1"}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":"first question"}}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"pondering"},{"type":"text","text":"working on it"},{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"ls"}}]}}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"file-a\nfile-b"}]}}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"done"}]}}"#,
        "\n",
        r#"{"type":"file-history-snapshot","messageId":"m1"}"#,
        "\n",
        r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"second question"}]}}"#,
        "\n",
        r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"second answer"}]}}"#,
    );

    fn records() -> Vec<Value> {
        parse_records(FIXTURE)
    }

    // --- encode_cwd ------------------------------------------------------

    #[test]
    fn encode_cwd_exact_cases() {
        assert_eq!(
            encode_cwd(Path::new("/Users/jinmu/Programming/cinchcli/relay/main")),
            "-Users-jinmu-Programming-cinchcli-relay-main"
        );
        assert_eq!(encode_cwd(Path::new("/a/b")), "-a-b");
        assert_eq!(encode_cwd(Path::new("/")), "-");
    }

    // --- user record classification --------------------------------------

    #[test]
    fn classifies_string_content_user_as_real_prompt() {
        let r: Value =
            serde_json::from_str(r#"{"type":"user","message":{"content":"hi"}}"#).unwrap();
        assert!(is_real_user_prompt(&r));
        assert!(!is_tool_result_user(&r));
        assert_eq!(user_prompt_text(&r).as_deref(), Some("hi"));
    }

    #[test]
    fn classifies_text_array_user_as_real_prompt() {
        let r: Value = serde_json::from_str(
            r#"{"type":"user","message":{"content":[{"type":"text","text":"hello"}]}}"#,
        )
        .unwrap();
        assert!(is_real_user_prompt(&r));
        assert!(!is_tool_result_user(&r));
        assert_eq!(user_prompt_text(&r).as_deref(), Some("hello"));
    }

    #[test]
    fn classifies_tool_result_only_user_as_continuation() {
        let r: Value = serde_json::from_str(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":"out"}]}}"#,
        )
        .unwrap();
        assert!(!is_real_user_prompt(&r));
        assert!(is_tool_result_user(&r));
    }

    #[test]
    fn ismeta_user_is_not_a_real_prompt() {
        // Claude injects skill text / caveats as `user` turns shaped like a
        // real prompt (text array) but flagged isMeta. These must not count.
        let r: Value = serde_json::from_str(
            r#"{"type":"user","isMeta":true,"message":{"content":[{"type":"text","text":"Brainstorming skill text"}]}}"#,
        )
        .unwrap();
        assert!(is_injected_meta(&r));
        assert!(!is_real_user_prompt(&r));
    }

    #[test]
    fn sidechain_record_is_injected_meta() {
        let r: Value = serde_json::from_str(
            r#"{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"sub-agent reply"}]}}"#,
        )
        .unwrap();
        assert!(is_injected_meta(&r));
    }

    #[test]
    fn meta_injection_neither_splits_nor_pollutes_an_answer() {
        // real prompt → assistant text → injected meta user turn → assistant
        // continues. Expect ONE answer whose parts are both assistant texts,
        // with the injected skill text nowhere in it.
        let recs = parse_records(
            r#"{"type":"user","message":{"content":"the only real question"}}
{"type":"assistant","message":{"content":[{"type":"text","text":"part one"}]}}
{"type":"user","isMeta":true,"message":{"content":[{"type":"text","text":"INJECTED SKILL TEXT"}]}}
{"type":"assistant","message":{"content":[{"type":"text","text":"part two"}]}}
{"type":"assistant","isSidechain":true,"message":{"content":[{"type":"text","text":"SUBAGENT NOISE"}]}}"#,
        );
        let answers = group_answers(&recs);
        assert_eq!(
            answers.len(),
            1,
            "meta injection must not start a new answer"
        );
        assert_eq!(
            answers[0].prompt.as_ref().map(|p| p.text.as_str()),
            Some("the only real question")
        );
        let texts: Vec<&str> = answers[0]
            .parts
            .iter()
            .filter_map(|p| match p {
                AnswerPart::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["part one", "part two"]);
        let rendered = format!("{answers:?}");
        assert!(!rendered.contains("INJECTED SKILL TEXT"));
        assert!(!rendered.contains("SUBAGENT NOISE"));
    }

    #[test]
    fn derive_title_ignores_leading_meta_injection() {
        let recs = parse_records(
            r#"{"type":"user","isMeta":true,"message":{"content":[{"type":"text","text":"INJECTED SKILL TEXT"}]}}
{"type":"user","message":{"content":"the real first prompt"}}
{"type":"assistant","message":{"content":[{"type":"text","text":"ok"}]}}"#,
        );
        assert_eq!(
            derive_title(&recs).as_deref(),
            Some("the real first prompt")
        );
    }

    // --- answer grouping -------------------------------------------------

    #[test]
    fn groups_into_two_answers_in_order() {
        let answers = group_answers(&records());
        assert_eq!(answers.len(), 2, "two real prompts → two answers");
        assert_eq!(answers[0].index, 0);
        assert_eq!(answers[1].index, 1);
        assert_eq!(
            answers[0].prompt.as_ref().map(|p| p.text.as_str()),
            Some("first question")
        );
        assert_eq!(
            answers[1].prompt.as_ref().map(|p| p.text.as_str()),
            Some("second question")
        );
    }

    #[test]
    fn first_answer_parts_in_source_order_and_tool_result_does_not_split() {
        let answers = group_answers(&records());
        let parts = &answers[0].parts;
        // thinking, text, tool_use, tool_result, text
        assert!(matches!(parts[0], AnswerPart::Thinking(_)));
        assert!(matches!(parts[1], AnswerPart::Text(ref t) if t == "working on it"));
        assert!(matches!(parts[2], AnswerPart::ToolUse { ref name, .. } if name == "Bash"));
        assert!(
            matches!(parts[3], AnswerPart::ToolResult { ref truncated_text } if truncated_text == "file-a\nfile-b")
        );
        assert!(matches!(parts[4], AnswerPart::Text(ref t) if t == "done"));
        assert_eq!(parts.len(), 5);
    }

    #[test]
    fn thinking_block_parsed() {
        let answers = group_answers(&records());
        assert!(matches!(
            answers[0].parts[0],
            AnswerPart::Thinking(ref t) if t == "pondering"
        ));
    }

    #[test]
    fn tool_use_input_is_compact_json() {
        let answers = group_answers(&records());
        let AnswerPart::ToolUse { ref input, .. } = answers[0].parts[2] else {
            panic!("expected tool_use");
        };
        assert_eq!(input, r#"{"command":"ls"}"#);
    }

    #[test]
    fn data_uri_tool_result_becomes_attachment() {
        let r: Value = serde_json::from_str(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":"data:image/png;base64,AAAABBBBCCCC"}]}}"#,
        )
        .unwrap();
        let mut a = Answer {
            index: 0,
            prompt: None,
            parts: Vec::new(),
        };
        push_tool_results(&mut a, &r);
        assert_eq!(a.parts.len(), 1);
        assert!(matches!(
            a.parts[0],
            AnswerPart::Attachment { ref label } if label == "image.png"
        ));
    }

    #[test]
    fn plain_text_tool_result_is_kept() {
        let r: Value = serde_json::from_str(
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":"hello, this is normal command output"}]}}"#,
        )
        .unwrap();
        let mut a = Answer {
            index: 0,
            prompt: None,
            parts: Vec::new(),
        };
        push_tool_results(&mut a, &r);
        assert!(matches!(a.parts[0], AnswerPart::ToolResult { .. }));
    }

    // --- title derivation ------------------------------------------------

    #[test]
    fn derive_title_prefers_ai_title() {
        assert_eq!(derive_title(&records()).as_deref(), Some("Fix the parser"));
    }

    #[test]
    fn derive_title_falls_back_to_first_prompt() {
        let no_ai: Vec<Value> = records()
            .into_iter()
            .filter(|r| record_type(r) != Some("ai-title"))
            .collect();
        assert_eq!(derive_title(&no_ai).as_deref(), Some("first question"));
    }

    // --- meta records skipped -------------------------------------------

    #[test]
    fn meta_records_are_not_answers_or_parts() {
        let answers = group_answers(&records());
        // Only two real prompts → two answers, regardless of the mode /
        // permission-mode / ai-title / file-history-snapshot lines.
        assert_eq!(answers.len(), 2);
        // No part should have leaked from a meta record.
        let total_parts: usize = answers.iter().map(|a| a.parts.len()).sum();
        assert_eq!(total_parts, 6); // 5 in answer 0 + 1 in answer 1
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let text = concat!(
            r#"{"type":"user","message":{"content":"ok"}}"#,
            "\n",
            "this is not json",
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"hi"}]}}"#,
        );
        let recs = parse_records(text);
        assert_eq!(recs.len(), 2);
    }
}
