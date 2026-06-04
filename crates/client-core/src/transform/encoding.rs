//! Percent-encoding (URL encode/decode) helpers for `transform`.

use super::TransformError;

pub(super) fn percent_encode(input: &str) -> String {
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

pub(super) fn percent_decode(input: &str) -> Result<String, TransformError> {
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
