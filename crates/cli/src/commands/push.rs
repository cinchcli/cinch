//! `cinch push` — read stdin, save to local clip history.
//!
//! Ingests content from stdin and stores it in the local database. This
//! command is local-only; the relay is never contacted.

use std::io::Read;
use std::time::Instant;

use client_core::auth::load_config;
use client_core::config::Config;
use client_core::machine::hostname_or_unknown;
use client_core::rest::ContentType;
use client_core::store::models::{StoredClip, SyncState};
use client_core::store::{self, queries, Store};

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

const MAX_PUSH_SIZE: usize = 20 * 1024 * 1024;

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Label for this clip.
    #[arg(short = 'l', long)]
    pub label: Option<String>,

    /// Suppress success output.
    #[arg(short = 's', long)]
    pub silent: bool,

    /// Force content type. Accepts `image` or any `image/*` MIME to override
    /// the image-vs-text decision; text subtypes (text/url/code) are derived
    /// automatically by `client_core::classify::detect`.
    #[arg(long = "type")]
    pub force_type: Option<String>,

    /// Force text mode (skip binary detection).
    #[arg(long)]
    pub text: bool,

    /// Override auth token (ignored in local-only mode).
    #[arg(long)]
    pub token: Option<String>,

    /// Override relay URL (ignored in local-only mode).
    #[arg(long)]
    pub relay: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailedType {
    Text,
    Image,
    Video,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    // Note: no `ensure_authenticated()` guard here. `resolve_config` below
    // overlays `--token` and `CINCH_TOKEN` on top of disk state and then
    // emits the same `AUTH_FAILURE` + `Run: cinch auth login` error when
    // every source is empty, so adding the guard would override the
    // documented stateless-push path (CI / containers without `~/.cinch`).
    let _cfg = resolve_config(&args)?;

    let mut data = Vec::new();
    std::io::stdin()
        .read_to_end(&mut data)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Cannot read stdin: {}", e), ""))?;

    if data.is_empty() {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "No input. Pipe content to cinch push.",
            "Example: echo 'hello' | cinch push",
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

    let hostname = hostname_or_unknown();
    let source = format!("remote:{}", hostname);

    let is_binary = if args.text {
        false
    } else if let Some(ft) = &args.force_type {
        force_is_image(ft)
    } else {
        matches!(detected, DetailedType::Image)
    };
    let wire_type = if is_binary {
        ContentType::Image
    } else if args.text {
        ContentType::Text
    } else {
        client_core::classify::detect(&data)
    };

    let start = Instant::now();
    let original_size = data.len() as i64;

    let store_path = store::default_db_path().map_err(|e| {
        ExitError::new(
            GENERIC_ERROR,
            format!("Could not determine local store path: {}", e),
            "",
        )
    })?;
    let store = Store::open(&store_path).map_err(|e| {
        ExitError::new(
            GENERIC_ERROR,
            format!("Could not open local store: {}", e),
            "",
        )
    })?;

    let clip_id = ulid::Ulid::new().to_string();
    let stored = StoredClip {
        id: clip_id.clone(),
        source: source.to_string(),
        source_key: None,
        source_app_id: None,
        source_app: None,
        source_url: None,
        label: args.label,
        content_type: wire_type.as_wire().to_string(),
        content: Some(data),
        media_path: None,
        byte_size: original_size,
        created_at: chrono::Utc::now().timestamp_millis(),
        pinned: false,
        pinned_at: None,
        sync_state: SyncState::Local,
    };

    queries::insert_clip(&store, &stored).map_err(|e| {
        ExitError::new(
            GENERIC_ERROR,
            format!("Local store write failed: {}", e),
            "",
        )
    })?;

    let signal_path = store_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new(""))
        .join("local_push.signal");
    let _ = std::fs::write(&signal_path, b"1");

    if !args.silent {
        eprintln!(
            "\u{2713} Saved {} locally (id={}) \u{00B7} {} ms",
            format_bytes(original_size),
            clip_id,
            start.elapsed().as_millis()
        );
    }
    Ok(())
}

/// `--type` accepts either canonical `image` or any `image/*` MIME for
/// backwards compatibility with prior CLI invocations.
fn force_is_image(s: &str) -> bool {
    s == "image" || s.starts_with("image/")
}

/// Sniffs image or video magic bytes; falls back to Text. The Text return
/// is then refined into Text / Url / Code by `client_core::classify::detect`.
fn detect_content_type(data: &[u8]) -> DetailedType {
    // Image sniffing is shared via `client_core::media` (single source of
    // truth across CLI + desktop). Video detection stays local — video isn't
    // part of the wire content_type vocabulary.
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

fn format_bytes(n: i64) -> String {
    let f = n as f64;
    if f >= 1024.0 * 1024.0 {
        format!("{:.1} MB", f / (1024.0 * 1024.0))
    } else if f >= 1024.0 {
        format!("{:.1} KB", f / 1024.0)
    } else {
        format!("{} B", n)
    }
}

fn resolve_config(args: &Args) -> Result<Config, ExitError> {
    if let (Some(token), Some(relay)) = (args.token.as_ref(), args.relay.as_ref()) {
        return Ok(Config {
            token: token.clone(),
            relay_url: relay.trim_end_matches('/').to_string(),
            ..Config::default()
        });
    }
    let mut cfg = load_config().map_err(|e| {
        ExitError::new(
            AUTH_FAILURE,
            format!("Could not load config: {}", e),
            "Run: cinch auth login",
        )
    })?;
    if let Some(token) = &args.token {
        cfg.token = token.clone();
    }
    if let Some(relay) = &args.relay {
        cfg.relay_url = relay.trim_end_matches('/').to_string();
    }
    if let Ok(env_token) = std::env::var("CINCH_TOKEN") {
        if !env_token.is_empty() {
            cfg.token = env_token;
        }
    }
    if let Ok(env_relay) = std::env::var("CINCH_RELAY_URL") {
        if !env_relay.is_empty() {
            cfg.relay_url = env_relay.trim_end_matches('/').to_string();
        }
    }
    if cfg.token.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "No auth token configured.",
            "Run: cinch auth login",
        ));
    }
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_with(token: Option<&str>, relay: Option<&str>) -> Args {
        Args {
            label: None,
            silent: false,
            force_type: None,
            text: false,
            token: token.map(String::from),
            relay: relay.map(String::from),
        }
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

    #[test]
    fn resolve_config_with_both_flags_short_circuits_disk() {
        // When --token AND --relay are both passed, resolve_config returns
        // without touching ~/.cinch/config.json. Verify the early-return
        // path AND that the relay URL's trailing slash is stripped.
        let args = args_with(Some("tok-xyz"), Some("https://relay.example/"));
        let cfg = resolve_config(&args).expect("args short-circuit succeeds");
        assert_eq!(cfg.token, "tok-xyz");
        assert_eq!(cfg.relay_url, "https://relay.example");
    }
}
