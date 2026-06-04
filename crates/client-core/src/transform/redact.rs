//! Secret redaction for `transform` — JSON-aware, with a line-based text fallback.

use serde_json::Value;

pub(super) fn redact_secrets(input: &str) -> String {
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
