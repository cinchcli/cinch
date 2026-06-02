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
/// Answers with no content under `opts` (e.g. an in-progress trailing turn, or
/// a thinking-only answer when thinking is excluded) are skipped entirely — no
/// lone `## Assistant` heading and no stray separators. Remaining answers are
/// separated by a `---` rule. The output ends with exactly one trailing
/// newline (so a fully-empty selection renders as just `"\n"`).
pub fn markdown(answers: &[Answer], opts: RenderOpts) -> String {
    let sections: Vec<String> = answers
        .iter()
        .filter_map(|a| render_answer(a, opts))
        .collect();
    let mut out = sections.join("\n---\n\n");
    // Trim trailing blank lines down to exactly one newline.
    while out.ends_with('\n') {
        out.pop();
    }
    out.push('\n');
    out
}

/// Render a single answer's Markdown section, or `None` when it has no content
/// under `opts`. [`answer_is_empty`] is the public predicate for this.
fn render_answer(answer: &Answer, opts: RenderOpts) -> Option<String> {
    let mut body = String::new();
    for part in &answer.parts {
        match part {
            AnswerPart::Text(t) => {
                if !t.trim().is_empty() {
                    body.push_str(t);
                    body.push_str("\n\n");
                }
            }
            AnswerPart::Thinking(t) => {
                if opts.include_thinking && !t.trim().is_empty() {
                    body.push_str(&fenced("thinking", t));
                }
            }
            AnswerPart::ToolUse { name, input } => {
                if opts.include_tools {
                    body.push_str(&fenced(&format!("tool: {name}"), input));
                }
            }
            AnswerPart::ToolResult { truncated_text } => {
                if opts.include_tools {
                    let truncated = truncate(truncated_text, opts.tool_result_max);
                    body.push_str(&fenced("tool-result", &truncated));
                }
            }
            AnswerPart::Attachment { label } => {
                body.push_str(&format!("[attachment: {label}]\n\n"));
            }
        }
    }

    let prompt_md = if opts.with_prompt {
        answer
            .prompt
            .as_ref()
            .filter(|p| !p.text.trim().is_empty())
            .map(|p| format!("## User\n\n{}\n\n", p.text))
    } else {
        None
    };

    // Nothing to show: no assistant body and (without --with-prompt) no prompt.
    if body.trim().is_empty() && prompt_md.is_none() {
        return None;
    }

    let mut section = String::new();
    if let Some(prompt) = prompt_md {
        section.push_str(&prompt);
    }
    section.push_str("## Assistant\n\n");
    section.push_str(&body);
    Some(section)
}

/// True when `answer` renders to no copyable content under `opts`. Callers use
/// this to skip in-progress / empty turns when selecting answers to copy.
pub fn answer_is_empty(answer: &Answer, opts: RenderOpts) -> bool {
    render_answer(answer, opts).is_none()
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

    #[test]
    fn answer_is_empty_skips_in_progress_and_thinking_only() {
        let d = RenderOpts::default();
        // No assistant parts, prompt present but --with-prompt off → empty.
        assert!(answer_is_empty(&answer(vec![]), d));
        // With --with-prompt, the prompt itself is copyable content.
        assert!(!answer_is_empty(
            &answer(vec![]),
            RenderOpts {
                with_prompt: true,
                ..d
            }
        ));
        // Thinking-only is empty by default, non-empty when thinking is shown.
        let thinking = answer(vec![AnswerPart::Thinking("plan".into())]);
        assert!(answer_is_empty(&thinking, d));
        assert!(!answer_is_empty(
            &thinking,
            RenderOpts {
                include_thinking: true,
                ..d
            }
        ));
        // Any real text is content.
        assert!(!answer_is_empty(
            &answer(vec![AnswerPart::Text("hi".into())]),
            d
        ));
    }

    #[test]
    fn empty_answers_are_skipped_in_markdown() {
        let empty = Answer {
            index: 0,
            prompt: None,
            parts: vec![],
        };
        let real = Answer {
            index: 1,
            prompt: None,
            parts: vec![AnswerPart::Text("kept".into())],
        };
        let md = markdown(&[empty.clone(), real, empty], RenderOpts::default());
        assert!(md.contains("kept"));
        // No lone heading and no stray separators around skipped empties.
        assert!(!md.contains("---"));
        assert_eq!(md.matches("## Assistant").count(), 1);
    }

    #[test]
    fn all_empty_selection_renders_just_newline() {
        let empty = Answer {
            index: 0,
            prompt: None,
            parts: vec![],
        };
        assert_eq!(markdown(&[empty], RenderOpts::default()), "\n");
    }
}
