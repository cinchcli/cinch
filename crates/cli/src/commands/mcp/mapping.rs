//! Convert internal `StoredClip` rows into the MCP-facing JSON shape.

use client_core::store::models::StoredClip;
use serde::Serialize;

/// Max characters of text returned in list/search previews.
pub const PREVIEW_CHARS: usize = 280;

#[derive(Debug, Serialize, PartialEq)]
pub struct McpClip {
    pub id: String,
    /// Text content. `None` for image clips (raw bytes are never serialized).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    pub content_type: String,
    pub source: String,
    /// ISO 8601 / RFC 3339 timestamp.
    pub created_at: String,
    pub byte_size: i64,
    /// True when `content` was truncated to a preview.
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_path: Option<String>,
}

/// Collapse legacy MIME-style content types to cinch's canonical vocabulary.
/// `text/*` -> "text", `image/*` -> "image"; canonical values pass through.
pub fn normalize_content_type(ct: &str) -> String {
    if ct.starts_with("image") {
        "image".to_string()
    } else if ct.starts_with("text") {
        "text".to_string()
    } else {
        ct.to_string()
    }
}

/// Convert a stored clip into the MCP-facing shape.
/// `full = false` truncates text to a preview (list/search);
/// `full = true` returns the whole text (`get_clipboard_item`).
/// Image clips never return bytes — only metadata + `media_path`.
pub fn to_mcp_clip(c: &StoredClip, full: bool) -> McpClip {
    let content_type = normalize_content_type(&c.content_type);
    let (content, truncated) = if content_type == "image" {
        (None, false)
    } else {
        match &c.content {
            None => (None, false),
            Some(bytes) => {
                let text = String::from_utf8_lossy(bytes);
                if full {
                    (Some(text.into_owned()), false)
                } else {
                    let mut chars = text.chars();
                    let preview: String = chars.by_ref().take(PREVIEW_CHARS).collect();
                    let truncated = chars.next().is_some();
                    (Some(preview), truncated)
                }
            }
        }
    };
    McpClip {
        id: c.id.clone(),
        content,
        content_type,
        source: c.source.clone(),
        created_at: iso8601_from_unix_ms(c.created_at),
        byte_size: c.byte_size,
        truncated,
        media_path: c.media_path.clone(),
    }
}

fn iso8601_from_unix_ms(ms: i64) -> String {
    use chrono::{TimeZone, Utc};
    Utc.timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(content_type: &str, content: Option<&str>) -> StoredClip {
        StoredClip {
            id: "01HX".to_string(),
            source: "remote:macbook".to_string(),
            source_key: None,
            content_type: content_type.to_string(),
            content: content.map(|s| s.as_bytes().to_vec()),
            media_path: None,
            byte_size: content.map(|s| s.len() as i64).unwrap_or(0),
            created_at: 1_700_000_000_000, // unix ms
            pinned: false,
            pinned_at: None,
            synced: true,
        }
    }

    #[test]
    fn normalizes_legacy_mime() {
        assert_eq!(normalize_content_type("text/plain"), "text");
        assert_eq!(normalize_content_type("image/png"), "image");
        assert_eq!(normalize_content_type("code"), "code");
        assert_eq!(normalize_content_type("url"), "url");
    }

    #[test]
    fn short_text_is_full_not_truncated() {
        let m = to_mcp_clip(&clip("text", Some("hello")), false);
        assert_eq!(m.content.as_deref(), Some("hello"));
        assert!(!m.truncated);
    }

    #[test]
    fn long_text_is_truncated_when_not_full() {
        let long = "x".repeat(PREVIEW_CHARS + 50);
        let m = to_mcp_clip(&clip("text", Some(&long)), false);
        assert!(m.truncated);
        assert_eq!(m.content.as_ref().unwrap().chars().count(), PREVIEW_CHARS);
    }

    #[test]
    fn long_text_is_complete_when_full() {
        let long = "x".repeat(PREVIEW_CHARS + 50);
        let m = to_mcp_clip(&clip("text", Some(&long)), true);
        assert!(!m.truncated);
        assert_eq!(
            m.content.as_ref().unwrap().chars().count(),
            PREVIEW_CHARS + 50
        );
    }

    #[test]
    fn image_returns_no_bytes() {
        let mut c = clip("image/png", Some("PNGBYTES"));
        c.media_path = Some("/tmp/x.png".to_string());
        let m = to_mcp_clip(&c, true);
        assert_eq!(m.content, None);
        assert_eq!(m.content_type, "image");
        assert_eq!(m.media_path.as_deref(), Some("/tmp/x.png"));
    }

    #[test]
    fn created_at_is_rfc3339() {
        let m = to_mcp_clip(&clip("text", Some("hi")), false);
        assert!(m.created_at.starts_with("2023-11-"));
    }
}
