//! Markdown code-block wrapping for `transform`.

pub(super) fn markdown_code_block(input: &str, content_type: &str) -> String {
    let fence = markdown_backtick_fence(input);
    format!(
        "{}{}\n{}\n{}",
        fence,
        markdown_language_hint(content_type),
        input,
        fence
    )
}

fn markdown_backtick_fence(input: &str) -> String {
    let mut longest_run = 0usize;
    let mut current_run = 0usize;

    for ch in input.chars() {
        if ch == '`' {
            current_run += 1;
            longest_run = longest_run.max(current_run);
        } else {
            current_run = 0;
        }
    }

    "`".repeat(3usize.max(longest_run + 1))
}

fn markdown_language_hint(content_type: &str) -> &'static str {
    let normalized = content_type
        .split_once(';')
        .map_or(content_type, |(value, _)| value)
        .trim()
        .to_ascii_lowercase();

    match normalized.as_str() {
        "json" | "text/json" => "json",
        _ => "text",
    }
}
