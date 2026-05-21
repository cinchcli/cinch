//! Content classification for clipboard input.
//!
//! Returns `Text`, `Url`, or `Code`. The caller must have already ruled out
//! image bytes via magic-byte sniffing — this module never returns `Image`.
//!
//! Input is raw bytes: non-UTF-8 input short-circuits to `Text`, and input
//! over `MAX_CLASSIFY_BYTES` short-circuits to `Text` without any UTF-8
//! validation. This keeps `cinch push <huge-text>` (20 MB stdin) cheap —
//! no O(n) UTF-8 walk before bailing.
//!
//! Decision order (first match wins):
//!   1. > 64 KB bytes → Text (no UTF-8 scan)
//!   2. invalid UTF-8 → Text
//!   3. trim; empty → Text
//!   4. shebang `#!/...` → Code
//!   5. whole-string URL parse with scheme allow-list → Url
//!   6. `{...}` / `[...]` shape + valid JSON → Code
//!   7. any line starts with a code-opener keyword → Code
//!   8. symbol-to-alphanumeric ratio > 0.20 with at least one code bigram → Code
//!   9. ≥ 2 distinct code bigrams → Code
//!  10. indented line(s) with a code bigram → Code
//!  11. otherwise → Text

use crate::rest::ContentType;

const MAX_CLASSIFY_BYTES: usize = 64 * 1024;
const SYMBOL_RATIO_THRESHOLD: f32 = 0.20;

const ALLOWED_URL_SCHEMES: &[&str] = &[
    "http", "https", "ftp", "ftps", "ssh", "sftp", "mailto", "file", "ws", "wss",
];

/// Tokens that, when they begin a (left-trimmed) line, are unambiguous code
/// signals. The trailing space / paren prevents prose collisions like
/// "use this" or "let me know".
const CODE_LINE_OPENERS: &[&str] = &[
    "fn ",
    "def ",
    "function ",
    "function(",
    "class ",
    "interface ",
    "trait ",
    "impl ",
    "struct ",
    "enum ",
    "type ",
    "import ",
    "from ",
    "export ",
    "module ",
    "package ",
    "use ",
    "namespace ",
    "const ",
    "let ",
    "var ",
    "pub ",
    "static ",
    "async ",
    "await ",
    "return ",
    "yield ",
    "throw ",
    "if (",
    "for (",
    "while (",
    "switch (",
    "catch (",
    "#include",
    "#define",
    "#!/",
];

/// Token pairs almost never seen in natural-language prose.
const CODE_BIGRAMS: &[&str] = &[
    "=>", "->", "::", "!=", "==", "&&", "||", "</", "/>", "//", "/*", "*/", "++", "--", ">=", "<=",
    ">>", "<<", "...",
];

/// Classify a clip from raw bytes. Never returns `Image`.
///
/// Bytes input avoids an O(n) UTF-8 scan on large clipboard payloads:
/// callers can pass `&Vec<u8>` directly (e.g. `cinch push` stdin), and this
/// function caps both the byte buffer and the UTF-8 validation at
/// `MAX_CLASSIFY_BYTES` — anything past that boundary cannot affect the
/// classification decision anyway.
pub fn detect(content: &[u8]) -> ContentType {
    // Oversize bytes short-circuit to Text, preserving the prior
    // ">64 KB → Text" semantic without touching the buffer.
    if content.len() > MAX_CLASSIFY_BYTES {
        return ContentType::Text;
    }
    // Genuinely binary / non-UTF-8 input: caller should have caught image
    // bytes via magic-byte sniffing; everything else degrades to Text.
    let s = match std::str::from_utf8(content) {
        Ok(s) => s,
        Err(_) => return ContentType::Text,
    };
    detect_str(s)
}

fn detect_str(content: &str) -> ContentType {
    let s = content.trim();
    if s.is_empty() {
        return ContentType::Text;
    }

    if s.starts_with("#!/") {
        return ContentType::Code;
    }

    if !s.chars().any(char::is_whitespace) {
        if let Ok(url) = url::Url::parse(s) {
            if ALLOWED_URL_SCHEMES.contains(&url.scheme()) {
                return ContentType::Url;
            }
        }
    }

    let bytes = s.as_bytes();
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    if ((first == b'{' && last == b'}') || (first == b'[' && last == b']'))
        && serde_json::from_str::<serde_json::Value>(s).is_ok()
    {
        return ContentType::Code;
    }

    for line in s.lines() {
        let trimmed = line.trim_start();
        if CODE_LINE_OPENERS.iter().any(|kw| trimmed.starts_with(kw)) {
            return ContentType::Code;
        }
    }

    let scan = scan(s);
    if scan.symbol_ratio > SYMBOL_RATIO_THRESHOLD && scan.bigram_count >= 1 {
        return ContentType::Code;
    }
    if scan.bigram_count >= 2 {
        return ContentType::Code;
    }
    if scan.indented_lines >= 1 && scan.bigram_count >= 1 {
        return ContentType::Code;
    }

    ContentType::Text
}

struct ScanResult {
    symbol_ratio: f32,
    bigram_count: usize,
    indented_lines: usize,
}

fn scan(s: &str) -> ScanResult {
    let bytes = s.as_bytes();
    let mut symbol_count: usize = 0;
    let mut alnum_count: usize = 0;
    let mut indented_lines: usize = 0;

    if is_indent_at(bytes, 0) {
        indented_lines += 1;
    }
    for (i, &b) in bytes.iter().enumerate() {
        if is_code_symbol(b) {
            symbol_count += 1;
        } else if b.is_ascii_alphanumeric() {
            alnum_count += 1;
        } else if b == b'\n' && is_indent_at(bytes, i + 1) {
            indented_lines += 1;
        }
    }

    let bigram_count = CODE_BIGRAMS.iter().filter(|p| s.contains(*p)).count();
    let symbol_ratio = if alnum_count == 0 {
        0.0
    } else {
        symbol_count as f32 / alnum_count as f32
    };

    ScanResult {
        symbol_ratio,
        bigram_count,
        indented_lines,
    }
}

const fn is_code_symbol(b: u8) -> bool {
    matches!(
        b,
        b'{' | b'}'
            | b'('
            | b')'
            | b'['
            | b']'
            | b';'
            | b'='
            | b'<'
            | b'>'
            | b'/'
            | b'\\'
            | b'|'
            | b'&'
            | b'*'
            | b'+'
            | b':'
    )
}

fn is_indent_at(bytes: &[u8], i: usize) -> bool {
    match bytes.get(i) {
        Some(b'\t') => true,
        Some(b' ') => matches!(bytes.get(i + 1), Some(b' ')),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Most cases are text-shaped, so route them through the public bytes API
    // via `as_bytes()`. Bytes-only paths (non-UTF-8, oversize) get their own
    // explicit tests at the bottom.
    fn detect(s: &str) -> ContentType {
        super::detect(s.as_bytes())
    }

    #[test]
    fn empty_is_text() {
        assert_eq!(detect(""), ContentType::Text);
        assert_eq!(detect("   \n\t "), ContentType::Text);
    }

    #[test]
    fn short_prose_is_text() {
        assert_eq!(detect("Hello world"), ContentType::Text);
        assert_eq!(
            detect("This is a normal sentence with a period."),
            ContentType::Text
        );
    }

    #[test]
    fn korean_prose_is_text() {
        assert_eq!(
            detect("안녕하세요. 오늘 회의는 3시입니다."),
            ContentType::Text
        );
    }

    #[test]
    fn long_prose_is_text() {
        let s = "The quick brown fox jumps over the lazy dog. \
                 This is a longer paragraph designed to test that prose, \
                 even with occasional punctuation like commas, periods, and \
                 apostrophes, does not cross the code threshold.";
        assert_eq!(detect(s), ContentType::Text);
    }

    #[test]
    fn https_url() {
        assert_eq!(detect("https://example.com"), ContentType::Url);
        assert_eq!(
            detect("https://example.com/path?q=1&r=2#frag"),
            ContentType::Url
        );
        assert_eq!(detect("  https://example.com  "), ContentType::Url);
    }

    #[test]
    fn other_schemes_url() {
        assert_eq!(detect("http://localhost:8080"), ContentType::Url);
        assert_eq!(detect("mailto:foo@bar.com"), ContentType::Url);
        assert_eq!(detect("ssh://user@host.com"), ContentType::Url);
        assert_eq!(detect("file:///tmp/x"), ContentType::Url);
        assert_eq!(
            detect("wss://relay.example.com/v1/stream"),
            ContentType::Url
        );
    }

    #[test]
    fn url_with_whitespace_is_not_url() {
        assert_eq!(
            detect("check out https://example.com today"),
            ContentType::Text
        );
    }

    #[test]
    fn bare_hostname_is_not_url() {
        assert_eq!(detect("example.com"), ContentType::Text);
        assert_eq!(detect("foo.bar.baz"), ContentType::Text);
    }

    #[test]
    fn windows_path_is_not_url() {
        assert_eq!(detect("c:\\users\\me\\file.txt"), ContentType::Text);
    }

    #[test]
    fn shebang_is_code() {
        assert_eq!(detect("#!/usr/bin/env bash\necho hello"), ContentType::Code);
        assert_eq!(detect("#!/bin/sh"), ContentType::Code);
    }

    #[test]
    fn json_object_is_code() {
        assert_eq!(detect(r#"{"key": "value"}"#), ContentType::Code);
        assert_eq!(
            detect(r#"{"nested": {"deep": [1, 2, 3]}, "ok": true}"#),
            ContentType::Code
        );
    }

    #[test]
    fn json_array_is_code() {
        assert_eq!(detect("[1, 2, 3]"), ContentType::Code);
    }

    #[test]
    fn json_shaped_but_invalid_is_text() {
        assert_eq!(detect("{not really json}"), ContentType::Text);
    }

    #[test]
    fn rust_snippet_is_code() {
        let s = "fn main() {\n    let x = 42;\n    println!(\"{}\", x);\n}";
        assert_eq!(detect(s), ContentType::Code);
    }

    #[test]
    fn python_snippet_is_code() {
        let s = "def greet(name):\n    return f\"hello, {name}\"\n\nprint(greet(\"world\"))";
        assert_eq!(detect(s), ContentType::Code);
    }

    #[test]
    fn typescript_snippet_is_code() {
        let s = "const add = (a: number, b: number) => a + b;\nexport { add };";
        assert_eq!(detect(s), ContentType::Code);
    }

    #[test]
    fn javascript_one_liner_is_code() {
        // Single-line const declaration — the kind of clip a developer
        // routinely copies from a tutorial.
        assert_eq!(
            detect("const foo = bar.map(x => x * 2);"),
            ContentType::Code
        );
    }

    #[test]
    fn import_statement_is_code() {
        assert_eq!(
            detect("import { useState } from 'react';"),
            ContentType::Code
        );
    }

    #[test]
    fn html_snippet_is_code() {
        let s = "<div class=\"foo\">\n  <span>hi</span>\n</div>";
        assert_eq!(detect(s), ContentType::Code);
    }

    #[test]
    fn c_include_is_code() {
        assert_eq!(detect("#include <stdio.h>"), ContentType::Code);
    }

    #[test]
    fn huge_input_skips_classification() {
        let huge = "a".repeat(MAX_CLASSIFY_BYTES + 1);
        assert_eq!(detect(&huge), ContentType::Text);
    }

    #[test]
    fn prose_with_arrow_is_text() {
        // A single `->` in conversational text shouldn't trigger code.
        assert_eq!(
            detect("Move the cursor -> click submit -> wait"),
            ContentType::Text
        );
    }

    #[test]
    fn prose_with_let_is_text() {
        // "let" inside prose (not as a line opener) stays text.
        assert_eq!(
            detect("Why don't you let me know when you're free."),
            ContentType::Text
        );
    }

    #[test]
    fn non_utf8_bytes_is_text() {
        // 0xC3 0x28: 0xC3 starts a 2-byte sequence but 0x28 is not a valid
        // continuation byte — invalid UTF-8.
        assert_eq!(super::detect(&[0xC3, 0x28]), ContentType::Text);
        // High-bit garbage that is not valid UTF-8 anywhere.
        assert_eq!(super::detect(&[0xFF, 0xFE, 0xFD]), ContentType::Text);
    }

    #[test]
    fn oversize_bytes_skip_utf8_scan() {
        // Even if the head is unambiguously code, oversize input bails to
        // Text without a UTF-8 walk over the full buffer.
        let mut huge = Vec::with_capacity(MAX_CLASSIFY_BYTES + 32);
        huge.extend_from_slice(b"fn main() { println!(\"x\"); }\n");
        huge.resize(MAX_CLASSIFY_BYTES + 1, b'a');
        assert_eq!(super::detect(&huge), ContentType::Text);
    }
}
