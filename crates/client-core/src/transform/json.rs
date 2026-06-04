//! JSON pretty-print, minify, and validation helpers for `transform`.

use super::TransformError;
use serde_json::Value;

pub(super) fn pretty_json(input: &str) -> Result<String, TransformError> {
    validate_json(input)?;
    Ok(format_json_pretty(input))
}

pub(super) fn minify_json(input: &str) -> Result<String, TransformError> {
    validate_json(input)?;
    Ok(format_json_minified(input))
}

fn validate_json(input: &str) -> Result<(), TransformError> {
    let _: Value =
        serde_json::from_str(input).map_err(|err| TransformError::InvalidInput(err.to_string()))?;
    Ok(())
}

fn format_json_pretty(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;
    let mut indent = 0usize;
    let mut expanded_containers = Vec::new();

    while let Some(ch) = chars.next() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch.is_whitespace() {
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                out.push(ch);
            }
            '{' | '[' => {
                out.push(ch);
                let closing = if ch == '{' { '}' } else { ']' };
                let expanded = next_non_ws(&mut chars) != Some(closing);
                expanded_containers.push(expanded);
                if expanded {
                    indent += 1;
                    push_json_newline(&mut out, indent);
                }
            }
            '}' | ']' => {
                if expanded_containers.pop().unwrap_or(false) {
                    indent = indent.saturating_sub(1);
                    push_json_newline(&mut out, indent);
                }
                out.push(ch);
            }
            ',' => {
                out.push(ch);
                push_json_newline(&mut out, indent);
            }
            ':' => out.push_str(": "),
            _ => out.push(ch),
        }
    }

    out
}

fn format_json_minified(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escaped = false;

    for ch in input.chars() {
        if in_string {
            out.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        if ch == '"' {
            in_string = true;
            out.push(ch);
        } else if !ch.is_whitespace() {
            out.push(ch);
        }
    }

    out
}

fn next_non_ws(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<char> {
    chars.clone().find(|ch| !ch.is_whitespace())
}

fn push_json_newline(out: &mut String, indent: usize) {
    out.push('\n');
    for _ in 0..indent {
        out.push_str("  ");
    }
}
