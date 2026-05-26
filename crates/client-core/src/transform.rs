use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::error::Error;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TransformAction {
    PrettyJson,
    MinifyJson,
    TrimWhitespace,
    CollapseWhitespace,
    ShellSingleQuote,
    MarkdownCodeBlock,
    UrlEncode,
    UrlDecode,
    RedactSecrets,
}

impl TransformAction {
    pub const ALL: [TransformAction; 9] = [
        TransformAction::PrettyJson,
        TransformAction::MinifyJson,
        TransformAction::TrimWhitespace,
        TransformAction::CollapseWhitespace,
        TransformAction::ShellSingleQuote,
        TransformAction::MarkdownCodeBlock,
        TransformAction::UrlEncode,
        TransformAction::UrlDecode,
        TransformAction::RedactSecrets,
    ];

    pub fn id(self) -> &'static str {
        match self {
            TransformAction::PrettyJson => "pretty-json",
            TransformAction::MinifyJson => "minify-json",
            TransformAction::TrimWhitespace => "trim-whitespace",
            TransformAction::CollapseWhitespace => "collapse-whitespace",
            TransformAction::ShellSingleQuote => "shell-single-quote",
            TransformAction::MarkdownCodeBlock => "markdown-code-block",
            TransformAction::UrlEncode => "url-encode",
            TransformAction::UrlDecode => "url-decode",
            TransformAction::RedactSecrets => "redact-secrets",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TransformAction::PrettyJson => "Pretty JSON",
            TransformAction::MinifyJson => "Minify JSON",
            TransformAction::TrimWhitespace => "Trim Whitespace",
            TransformAction::CollapseWhitespace => "Collapse Whitespace",
            TransformAction::ShellSingleQuote => "Shell Single Quote",
            TransformAction::MarkdownCodeBlock => "Markdown Code Block",
            TransformAction::UrlEncode => "URL Encode",
            TransformAction::UrlDecode => "URL Decode",
            TransformAction::RedactSecrets => "Redact Secrets",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "pretty-json" | "pretty_json" => Some(TransformAction::PrettyJson),
            "minify-json" | "minify_json" => Some(TransformAction::MinifyJson),
            "trim-whitespace" | "trim_whitespace" => Some(TransformAction::TrimWhitespace),
            "collapse-whitespace" | "collapse_whitespace" => {
                Some(TransformAction::CollapseWhitespace)
            }
            "shell-single-quote" | "shell_single_quote" => Some(TransformAction::ShellSingleQuote),
            "markdown-code-block" | "markdown_code_block" => {
                Some(TransformAction::MarkdownCodeBlock)
            }
            "url-encode" | "url_encode" => Some(TransformAction::UrlEncode),
            "url-decode" | "url_decode" => Some(TransformAction::UrlDecode),
            "redact-secrets" | "redact_secrets" => Some(TransformAction::RedactSecrets),
            _ => None,
        }
    }

    pub fn info(self) -> TransformActionInfo {
        TransformActionInfo {
            id: self.id(),
            label: self.label(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TransformActionInfo {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransformError {
    UnsupportedContentType(String),
    InvalidInput(String),
}

impl fmt::Display for TransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransformError::UnsupportedContentType(content_type) => {
                write!(f, "unsupported content type: {content_type}")
            }
            TransformError::InvalidInput(message) => write!(f, "invalid input: {message}"),
        }
    }
}

impl Error for TransformError {}

pub fn list_transform_actions(content_type: &str) -> Vec<TransformActionInfo> {
    if !is_text_like(content_type) {
        return Vec::new();
    }

    TransformAction::ALL
        .into_iter()
        .map(TransformAction::info)
        .collect()
}

pub fn apply_transform(
    action: TransformAction,
    input: &str,
    content_type: &str,
) -> Result<String, TransformError> {
    if !is_text_like(content_type) {
        return Err(TransformError::UnsupportedContentType(
            content_type.to_string(),
        ));
    }

    match action {
        TransformAction::PrettyJson => pretty_json(input),
        TransformAction::MinifyJson => minify_json(input),
        TransformAction::TrimWhitespace => Ok(trim_whitespace(input)),
        TransformAction::CollapseWhitespace => Ok(collapse_whitespace(input)),
        TransformAction::ShellSingleQuote => Ok(shell_single_quote(input)),
        TransformAction::MarkdownCodeBlock => Ok(markdown_code_block(input, content_type)),
        TransformAction::UrlEncode => Ok(percent_encode(input)),
        TransformAction::UrlDecode => percent_decode(input),
        TransformAction::RedactSecrets => Ok(redact_secrets(input)),
    }
}

fn is_text_like(content_type: &str) -> bool {
    let normalized = content_type
        .split_once(';')
        .map_or(content_type, |(value, _)| value)
        .trim()
        .to_ascii_lowercase();

    matches!(normalized.as_str(), "text" | "code" | "url" | "json")
        || normalized.starts_with("text/")
}

fn pretty_json(input: &str) -> Result<String, TransformError> {
    validate_json(input)?;
    Ok(format_json_pretty(input))
}

fn minify_json(input: &str) -> Result<String, TransformError> {
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

fn trim_whitespace(input: &str) -> String {
    input
        .trim()
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn shell_single_quote(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\"'\"'"))
}

fn markdown_code_block(input: &str, content_type: &str) -> String {
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

fn percent_encode(input: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";

    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[(byte >> 4) as usize]));
            out.push(char::from(HEX[(byte & 0x0f) as usize]));
        }
    }
    out
}

fn percent_decode(input: &str) -> Result<String, TransformError> {
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err(TransformError::InvalidInput(
                        "incomplete percent escape".to_string(),
                    ));
                }
                let high = hex_value(bytes[index + 1]).ok_or_else(|| {
                    TransformError::InvalidInput("invalid percent escape".to_string())
                })?;
                let low = hex_value(bytes[index + 2]).ok_or_else(|| {
                    TransformError::InvalidInput("invalid percent escape".to_string())
                })?;
                out.push((high << 4) | low);
                index += 3;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8(out).map_err(|err| TransformError::InvalidInput(err.to_string()))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn redact_secrets(input: &str) -> String {
    if let Ok(mut value) = serde_json::from_str::<Value>(input) {
        redact_json_value(&mut value);
        if let Ok(mut out) = serde_json::to_string_pretty(&value) {
            if let Some(terminator) = trailing_line_terminator(input) {
                out.push_str(terminator);
            }
            return out;
        }
    }

    redact_text_secrets(input)
}

fn trailing_line_terminator(input: &str) -> Option<&'static str> {
    if input.ends_with("\r\n") {
        Some("\r\n")
    } else if input.ends_with('\n') {
        Some("\n")
    } else {
        None
    }
}

fn redact_json_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if is_secret_key(&normalize_secret_key(key)) {
                    *value = Value::String("[REDACTED]".to_string());
                } else {
                    redact_json_value(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_json_value(value);
            }
        }
        _ => {}
    }
}

fn redact_text_secrets(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut start = 0usize;

    for (index, ch) in input.char_indices() {
        if ch == '\n' {
            out.push_str(&redact_secret_segment(&input[start..=index]));
            start = index + 1;
        }
    }

    if start < input.len() {
        out.push_str(&redact_secret_segment(&input[start..]));
    }

    out
}

fn redact_secret_segment(segment: &str) -> String {
    let (line, terminator) = split_line_terminator(segment);
    let mut out = redact_secret_line(line);
    out.push_str(terminator);
    out
}

fn split_line_terminator(segment: &str) -> (&str, &str) {
    if let Some(line) = segment.strip_suffix("\r\n") {
        (line, "\r\n")
    } else if let Some(line) = segment.strip_suffix('\n') {
        (line, "\n")
    } else {
        (segment, "")
    }
}

fn redact_secret_line(line: &str) -> String {
    let Some((separator_index, separator)) = find_assignment_separator(line) else {
        return line.to_string();
    };

    let key = normalize_secret_key(&line[..separator_index]);
    if !is_secret_key(&key) {
        return line.to_string();
    }

    let value = &line[separator_index + separator.len_utf8()..];
    let leading_value_space_len = value.len() - value.trim_start().len();

    format!(
        "{}{}{}[REDACTED]",
        &line[..separator_index],
        separator,
        &value[..leading_value_space_len]
    )
}

fn find_assignment_separator(line: &str) -> Option<(usize, char)> {
    line.char_indices().find(|(_, ch)| matches!(ch, '=' | ':'))
}

fn normalize_secret_key(key: &str) -> String {
    key.trim()
        .trim_matches(|ch| matches!(ch, '"' | '\'' | '`' | '{' | '}' | '[' | ']' | '(' | ')'))
        .chars()
        .map(|ch| match ch {
            '-' | ' ' => '_',
            _ => ch.to_ascii_lowercase(),
        })
        .collect()
}

fn is_secret_key(key: &str) -> bool {
    matches!(
        key,
        "api_key"
            | "apikey"
            | "token"
            | "access_token"
            | "refresh_token"
            | "client_secret"
            | "secret"
            | "password"
            | "passwd"
            | "private_key"
            | "x_api_key"
            | "authorization"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_json_formats_valid_json() {
        let out = apply_transform(TransformAction::PrettyJson, r#"{"b":2,"a":1}"#, "json").unwrap();
        assert_eq!(out, "{\n  \"b\": 2,\n  \"a\": 1\n}");
    }

    #[test]
    fn pretty_json_rejects_invalid_json() {
        let err = apply_transform(TransformAction::PrettyJson, "{nope", "text").unwrap_err();
        assert!(matches!(err, TransformError::InvalidInput(_)));
    }

    #[test]
    fn minify_json_compacts_valid_json() {
        let out = apply_transform(TransformAction::MinifyJson, "{\n  \"a\": 1\n}", "json").unwrap();
        assert_eq!(out, r#"{"a":1}"#);
    }

    #[test]
    fn trim_whitespace_trims_edges_and_line_ends() {
        let out =
            apply_transform(TransformAction::TrimWhitespace, "  a  \n b\t \n", "text").unwrap();
        assert_eq!(out, "a\n b");
    }

    #[test]
    fn collapse_whitespace_uses_single_spaces() {
        let out =
            apply_transform(TransformAction::CollapseWhitespace, "a \n\t b   c", "text").unwrap();
        assert_eq!(out, "a b c");
    }

    #[test]
    fn shell_single_quote_escapes_embedded_quotes() {
        let out = apply_transform(TransformAction::ShellSingleQuote, "can't", "text").unwrap();
        assert_eq!(out, "'can'\"'\"'t'");
    }

    #[test]
    fn markdown_code_block_uses_content_type_hint() {
        let out =
            apply_transform(TransformAction::MarkdownCodeBlock, "let x = 1;", "code").unwrap();
        assert_eq!(out, "```text\nlet x = 1;\n```");
    }

    #[test]
    fn markdown_code_block_uses_longer_fence_than_input_backticks() {
        let out = apply_transform(
            TransformAction::MarkdownCodeBlock,
            "before ``` after",
            "text",
        )
        .unwrap();
        assert_eq!(out, "````text\nbefore ``` after\n````");
    }

    #[test]
    fn url_encode_and_decode_roundtrip_utf8() {
        let enc = apply_transform(TransformAction::UrlEncode, "hello world/한글", "text").unwrap();
        assert_eq!(enc, "hello%20world%2F%ED%95%9C%EA%B8%80");
        let dec = apply_transform(TransformAction::UrlDecode, &enc, "text").unwrap();
        assert_eq!(dec, "hello world/한글");
    }

    #[test]
    fn redact_secrets_masks_common_assignments() {
        let out = apply_transform(
            TransformAction::RedactSecrets,
            "api_key = sk-1234567890abcdef\npassword: hunter2",
            "text",
        )
        .unwrap();
        assert!(out.contains("api_key = [REDACTED]"));
        assert!(out.contains("password: [REDACTED]"));
    }

    #[test]
    fn redact_secrets_recurses_through_json_objects() {
        for content_type in ["json", "text/json"] {
            let out = apply_transform(
                TransformAction::RedactSecrets,
                r#"{"nested":{"client_secret":"abc","keep":"ok"},"refresh_token":"def"}"#,
                content_type,
            )
            .unwrap();
            let value: Value = serde_json::from_str(&out).unwrap();
            assert_eq!(value["nested"]["client_secret"], "[REDACTED]");
            assert_eq!(value["nested"]["keep"], "ok");
            assert_eq!(value["refresh_token"], "[REDACTED]");
        }
    }

    #[test]
    fn redact_secrets_recurses_through_json_shaped_code() {
        let out = apply_transform(
            TransformAction::RedactSecrets,
            "{\"nested\":{\"client_secret\":\"abc\",\"keep\":\"ok\"}}\n",
            "code",
        )
        .unwrap();
        assert!(out.ends_with('\n'));
        let value: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["nested"]["client_secret"], "[REDACTED]");
        assert_eq!(value["nested"]["keep"], "ok");
    }

    #[test]
    fn redact_secrets_masks_quoted_and_variant_text_keys() {
        let out = apply_transform(
            TransformAction::RedactSecrets,
            "\"client_secret\": \"abc\"\n{x-api-key} = key\nprivate-key: -----BEGIN KEY-----",
            "text",
        )
        .unwrap();
        assert!(out.contains("\"client_secret\": [REDACTED]"));
        assert!(out.contains("{x-api-key} = [REDACTED]"));
        assert!(out.contains("private-key: [REDACTED]"));
    }

    #[test]
    fn redact_secrets_preserves_line_terminators() {
        let out = apply_transform(
            TransformAction::RedactSecrets,
            "plain\r\npassword: hunter2\nlast\n",
            "text",
        )
        .unwrap();
        assert_eq!(out, "plain\r\npassword: [REDACTED]\nlast\n");
    }

    #[test]
    fn image_content_type_is_unsupported() {
        let err = apply_transform(TransformAction::TrimWhitespace, "ignored", "image").unwrap_err();
        assert_eq!(
            err,
            TransformError::UnsupportedContentType("image".to_string())
        );
    }

    #[test]
    fn action_ids_roundtrip() {
        for action in TransformAction::ALL {
            assert_eq!(TransformAction::from_id(action.id()), Some(action));
        }
        assert_eq!(
            TransformAction::from_id("pretty-json"),
            Some(TransformAction::PrettyJson)
        );
        assert_eq!(
            TransformAction::from_id("pretty_json"),
            Some(TransformAction::PrettyJson)
        );
    }
}
