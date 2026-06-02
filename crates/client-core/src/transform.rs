//! Clipboard text transforms (pretty/minify JSON, whitespace, encoding, redaction).
//!
//! This module is the public surface: the [`TransformAction`] vocabulary, the
//! [`apply_transform`] dispatcher, and content-type gating. The actual
//! transform implementations live in focused submodules ([`json`], [`text`],
//! [`markdown`], [`encoding`], [`redact`]).

use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;

mod encoding;
mod json;
mod markdown;
mod redact;
mod text;

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
        TransformAction::PrettyJson => json::pretty_json(input),
        TransformAction::MinifyJson => json::minify_json(input),
        TransformAction::TrimWhitespace => Ok(text::trim_whitespace(input)),
        TransformAction::CollapseWhitespace => Ok(text::collapse_whitespace(input)),
        TransformAction::ShellSingleQuote => Ok(text::shell_single_quote(input)),
        TransformAction::MarkdownCodeBlock => {
            Ok(markdown::markdown_code_block(input, content_type))
        }
        TransformAction::UrlEncode => Ok(encoding::percent_encode(input)),
        TransformAction::UrlDecode => encoding::percent_decode(input),
        TransformAction::RedactSecrets => Ok(redact::redact_secrets(input)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

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
