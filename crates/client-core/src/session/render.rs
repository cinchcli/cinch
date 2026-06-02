//! Markdown renderer turning selected [`Answer`]s into clean Markdown.

use super::model::{Answer, AnswerPart};

/// Knobs controlling how answers are rendered to Markdown.
#[derive(Debug, Clone, Copy)]
pub struct RenderOpts {
    /// Include the eliciting user prompt (`## User`) above each answer.
    pub with_prompt: bool,
    /// Include `Thinking` parts (default off — noise for handoff).
    pub include_thinking: bool,
    /// Include `ToolUse` + `ToolResult` parts (default on).
    pub include_tools: bool,
    /// Max chars of a tool result before truncation.
    pub tool_result_max: usize,
}

impl Default for RenderOpts {
    fn default() -> Self {
        Self {
            with_prompt: false,
            include_thinking: false,
            include_tools: true,
            tool_result_max: 800,
        }
    }
}

/// Render `answers` in order to clean Markdown per `opts`.
///
/// Multiple answers are separated by a `---` rule. The output ends with
/// exactly one trailing newline.
pub fn markdown(answers: &[Answer], opts: RenderOpts) -> String {
    let mut out = String::new();
    for (i, answer) in answers.iter().enumerate() {
        if i > 0 {
            out.push_str("\n---\n\n");
        }
        if opts.with_prompt {
            if let Some(prompt) = &answer.prompt {
                out.push_str("## User\n\n");
                out.push_str(&prompt.text);
                out.push_str("\n\n");
            }
        }
        out.push_str("## Assistant\n\n");
        for part in &answer.parts {
            match part {
                AnswerPart::Text(t) => {
                    out.push_str(t);
                    out.push_str("\n\n");
                }
                AnswerPart::Thinking(t) => {
                    if opts.include_thinking && !t.trim().is_empty() {
                        out.push_str(&fenced("thinking", t));
                    }
                }
                AnswerPart::ToolUse { name, input } => {
                    if opts.include_tools {
                        out.push_str(&fenced(&format!("tool: {name}"), input));
                    }
                }
                AnswerPart::ToolResult { truncated_text } => {
                    if opts.include_tools {
                        let body = truncate(truncated_text, opts.tool_result_max);
                        out.push_str(&fenced("tool-result", &body));
                    }
                }
                AnswerPart::Attachment { label } => {
                    out.push_str(&format!("[attachment: {label}]\n\n"));
                }
            }
        }
    }
    // Trim trailing blank lines down to exactly one newline.
    while out.ends_with('\n') {
        out.pop();
    }
    out.push('\n');
    out
}

/// Truncate `s` to `max` chars (UTF-8 safe), appending a truncation note.
fn truncate(s: &str, max: usize) -> String {
    let total = s.chars().count();
    if total <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}\n… (truncated, {} more chars)", total - max)
}

/// Build a fenced code block with `lang` info string and `body` content.
fn fenced(lang: &str, body: &str) -> String {
    format!("```{lang}\n{body}\n```\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::model::Prompt;

    fn answer(parts: Vec<AnswerPart>) -> Answer {
        Answer {
            index: 0,
            prompt: Some(Prompt {
                text: "do the thing".into(),
            }),
            parts,
        }
    }

    #[test]
    fn default_opts_include_tools_exclude_thinking_no_user_heading() {
        let a = answer(vec![
            AnswerPart::Thinking("secret reasoning".into()),
            AnswerPart::Text("hi".into()),
            AnswerPart::ToolUse {
                name: "Bash".into(),
                input: "{\"command\":\"ls\"}".into(),
            },
        ]);
        let md = markdown(&[a], RenderOpts::default());
        assert!(!md.contains("## User"));
        assert!(!md.contains("secret reasoning"));
        assert!(md.contains("```tool: Bash"));
        assert!(md.contains("## Assistant"));
    }

    #[test]
    fn with_prompt_emits_user_heading() {
        let a = answer(vec![AnswerPart::Text("answer".into())]);
        let md = markdown(
            &[a],
            RenderOpts {
                with_prompt: true,
                ..RenderOpts::default()
            },
        );
        assert!(md.contains("## User\n\ndo the thing"));
    }

    #[test]
    fn include_thinking_emits_fence() {
        let a = answer(vec![AnswerPart::Thinking("the plan".into())]);
        let md = markdown(
            &[a],
            RenderOpts {
                include_thinking: true,
                ..RenderOpts::default()
            },
        );
        assert!(md.contains("```thinking\nthe plan\n```"));
    }

    #[test]
    fn no_tools_drops_tool_use_and_result() {
        let a = answer(vec![
            AnswerPart::ToolUse {
                name: "Bash".into(),
                input: "x".into(),
            },
            AnswerPart::ToolResult {
                truncated_text: "out".into(),
            },
            AnswerPart::Text("kept".into()),
        ]);
        let md = markdown(
            &[a],
            RenderOpts {
                include_tools: false,
                ..RenderOpts::default()
            },
        );
        assert!(!md.contains("tool: Bash"));
        assert!(!md.contains("tool-result"));
        assert!(md.contains("kept"));
    }

    #[test]
    fn long_tool_result_is_truncated() {
        let long = "x".repeat(50);
        let a = answer(vec![AnswerPart::ToolResult {
            truncated_text: long,
        }]);
        let md = markdown(
            &[a],
            RenderOpts {
                tool_result_max: 10,
                ..RenderOpts::default()
            },
        );
        assert!(md.contains("(truncated"));
    }

    #[test]
    fn attachment_renders_regardless_of_flags() {
        let a = answer(vec![AnswerPart::Attachment {
            label: "image.png".into(),
        }]);
        // Even with tools off, the placeholder still renders.
        let md = markdown(
            &[a],
            RenderOpts {
                include_tools: false,
                ..RenderOpts::default()
            },
        );
        assert!(md.contains("[attachment: image.png]"));
    }

    #[test]
    fn multi_answer_has_separator_and_order() {
        let a0 = Answer {
            index: 0,
            prompt: None,
            parts: vec![AnswerPart::Text("first".into())],
        };
        let a1 = Answer {
            index: 1,
            prompt: None,
            parts: vec![AnswerPart::Text("second".into())],
        };
        let md = markdown(&[a0, a1], RenderOpts::default());
        assert!(md.contains("\n---\n"));
        let first_at = md.find("first").unwrap();
        let second_at = md.find("second").unwrap();
        assert!(first_at < second_at, "answers preserve session order");
    }

    #[test]
    fn output_ends_with_single_newline() {
        let a = answer(vec![AnswerPart::Text("hi".into())]);
        let md = markdown(&[a], RenderOpts::default());
        assert!(md.ends_with('\n'));
        assert!(!md.ends_with("\n\n"));
    }
}
