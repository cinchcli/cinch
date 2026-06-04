//! Helpers shared across `cinch` subcommands.

use std::io::Read;

use client_core::protocol::DeviceInfo;
use client_core::rest::ContentType;

use crate::exit::{ExitError, GENERIC_ERROR};

/// Resolves a device nickname/hostname to its `source_key`.
///
/// Matching is case-insensitive against both `nickname` (when non-empty) and
/// `hostname`. Falls back to `remote:<from>` when no device matches. Shared by
/// `list` (operates on a pre-fetched slice) and `pull` (fetches, then matches).
pub fn match_device_source(devices: &[DeviceInfo], from: &str) -> String {
    let lower = from.to_lowercase();
    for d in devices {
        let nick_match = !d.nickname.is_empty() && d.nickname.to_lowercase() == lower;
        let host_match = d.hostname.to_lowercase() == lower;
        if nick_match || host_match {
            return d.source_key.clone();
        }
    }
    format!("remote:{}", from)
}

/// Maximum stdin payload accepted by `cinch push` / `cinch send` (20 MB).
pub const MAX_PUSH_SIZE: usize = 20 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailedType {
    Text,
    Image,
    Video,
}

/// `--type` accepts either canonical `image` or any `image/*` MIME for
/// backwards compatibility with prior CLI invocations.
fn force_is_image(s: &str) -> bool {
    s == "image" || s.starts_with("image/")
}

/// Sniffs image or video magic bytes; falls back to Text. The Text return is
/// then refined into Text / Url / Code by `client_core::classify::detect`.
fn detect_content_type(data: &[u8]) -> DetailedType {
    // Image sniffing is shared via `client_core::media` (single source of truth
    // across CLI + desktop). Video detection stays local — video isn't part of
    // the wire content_type vocabulary.
    if client_core::media::is_image(data) {
        return DetailedType::Image;
    }

    // Common video magic bytes:
    // MP4: ....ftyp (offset 4)
    // MOV: ....moov or ....ftyp
    // AVI: RIFF....AVI
    // MKV: \x1a\x45\xdf\xa3 (EBML)
    let is_video = (data.len() >= 12 && (&data[4..8] == b"ftyp" || &data[4..8] == b"moov"))
        || (data.starts_with(b"RIFF") && data.len() >= 12 && &data[8..12] == b"AVI ")
        || data.starts_with(b"\x1a\x45\xdf\xa3");

    if is_video {
        DetailedType::Video
    } else {
        DetailedType::Text
    }
}

/// Format a byte count as `B` / `KB` / `MB` for human-facing output.
pub fn format_bytes(n: i64) -> String {
    let f = n as f64;
    if f >= 1024.0 * 1024.0 {
        format!("{:.1} MB", f / (1024.0 * 1024.0))
    } else if f >= 1024.0 {
        format!("{:.1} KB", f / 1024.0)
    } else {
        format!("{} B", n)
    }
}

/// Read all of stdin, enforce the 20 MB cap, reject video, and classify the
/// payload into the canonical wire [`ContentType`]. Honors `--text` (force
/// text) and `--type` (force `image` / `image/*`); text subtypes (text / url /
/// code) are derived by `client_core::classify::detect`. `cmd` names the calling
/// verb (`push` / `send`) for the empty-input hint. Shared by `cinch push` and
/// `cinch send` so the read + classification logic has a single home.
pub fn read_and_classify_stdin(
    cmd: &str,
    force_text: bool,
    force_type: Option<&str>,
) -> Result<(Vec<u8>, ContentType), ExitError> {
    let mut data = Vec::new();
    std::io::stdin()
        .read_to_end(&mut data)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Cannot read stdin: {}", e), ""))?;

    if data.is_empty() {
        return Err(ExitError::new(
            GENERIC_ERROR,
            format!("No input. Pipe content to cinch {}.", cmd),
            format!("Example: echo 'hello' | cinch {}", cmd),
        ));
    }
    if data.len() > MAX_PUSH_SIZE {
        return Err(ExitError::new(
            GENERIC_ERROR,
            format!(
                "Input too large: {} (max 20MB).",
                format_bytes(data.len() as i64)
            ),
            "",
        ));
    }

    let detected = detect_content_type(&data);
    if matches!(detected, DetailedType::Video) {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "Video files are not supported.",
            "Cinch supports text, code, and images (PNG, JPEG, GIF, WEBP).",
        ));
    }

    let is_binary = if force_text {
        false
    } else if let Some(ft) = force_type {
        force_is_image(ft)
    } else {
        matches!(detected, DetailedType::Image)
    };
    let wire_type = if is_binary {
        ContentType::Image
    } else if force_text {
        ContentType::Text
    } else {
        client_core::classify::detect(&data)
    };

    Ok((data, wire_type))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_nickname_case_insensitive() {
        let dev = DeviceInfo {
            nickname: "Desktop".into(),
            hostname: "host-1".into(),
            source_key: "remote:dev-abc".into(),
            ..Default::default()
        };
        assert_eq!(match_device_source(&[dev], "desktop"), "remote:dev-abc");
    }

    #[test]
    fn falls_back_to_remote_prefix() {
        let devices: Vec<DeviceInfo> = vec![];
        assert_eq!(match_device_source(&devices, "ghost"), "remote:ghost");
    }

    // --- force_is_image -----------------------------------------------------

    #[test]
    fn force_is_image_matches_canonical_image() {
        assert!(force_is_image("image"));
    }

    #[test]
    fn force_is_image_matches_legacy_mime_subtypes() {
        // Pre-2026-05 callers passed `--type image/png`; that path must
        // keep working so existing scripts don't break.
        assert!(force_is_image("image/png"));
        assert!(force_is_image("image/jpeg"));
        assert!(force_is_image("image/webp"));
    }

    #[test]
    fn force_is_image_rejects_non_image() {
        assert!(!force_is_image("text"));
        assert!(!force_is_image("text/plain"));
        assert!(!force_is_image(""));
        assert!(!force_is_image("IMAGE")); // case-sensitive on purpose
    }

    // --- detect_content_type ------------------------------------------------

    #[test]
    fn detect_content_type_recognizes_png() {
        let png = b"\x89PNG\r\n\x1a\nIHDR\x00";
        assert!(matches!(detect_content_type(png), DetailedType::Image));
    }

    #[test]
    fn detect_content_type_recognizes_jpeg() {
        let jpeg = b"\xff\xd8\xff\xe0\x00\x10JFIF";
        assert!(matches!(detect_content_type(jpeg), DetailedType::Image));
    }

    #[test]
    fn detect_content_type_recognizes_gif87a_and_gif89a() {
        assert!(matches!(
            detect_content_type(b"GIF87a"),
            DetailedType::Image
        ));
        assert!(matches!(
            detect_content_type(b"GIF89a"),
            DetailedType::Image
        ));
    }

    #[test]
    fn detect_content_type_recognizes_webp() {
        // RIFF<size>WEBP — the `WEBP` marker at bytes 8..12 is load-bearing.
        let webp = b"RIFF\x24\x00\x00\x00WEBPVP8 ";
        assert!(matches!(detect_content_type(webp), DetailedType::Image));
    }

    #[test]
    fn detect_content_type_recognizes_mp4() {
        let mp4 = b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00";
        assert!(matches!(detect_content_type(mp4), DetailedType::Video));
    }

    #[test]
    fn detect_content_type_recognizes_mov() {
        let mov = b"\x00\x00\x00\x18moovqt  \x00\x00\x02\x00";
        assert!(matches!(detect_content_type(mov), DetailedType::Video));
    }

    #[test]
    fn detect_content_type_recognizes_avi() {
        let avi = b"RIFF\x00\x00\x00\x00AVI LIST";
        assert!(matches!(detect_content_type(avi), DetailedType::Video));
    }

    #[test]
    fn detect_content_type_recognizes_mkv() {
        let mkv = b"\x1a\x45\xdf\xa3\x01\x00\x00\x00";
        assert!(matches!(detect_content_type(mkv), DetailedType::Video));
    }

    #[test]
    fn detect_content_type_text_fallback() {
        assert!(matches!(detect_content_type(b"hello"), DetailedType::Text));
        assert!(matches!(detect_content_type(b""), DetailedType::Text));
        // RIFF without the WEBP/AVI marker must NOT be classified as image/video.
        assert!(matches!(
            detect_content_type(b"RIFF\0\0\0\0WAVEfmt "),
            DetailedType::Text
        ));
    }

    // --- format_bytes -------------------------------------------------------

    #[test]
    fn format_bytes_buckets() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1023), "1023 B");
        // Boundary: exactly 1 KiB crosses into KB formatting.
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
        // Boundary: exactly 1 MiB crosses into MB formatting.
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    }
}
