//! `cinch push` — read stdin, optionally encrypt, send to relay.
//!
//! Mirrors `cinch/cmd/push.go` flag-for-flag. Encryption uses the active
//! relay's `encryption_key` (base64url AES-256) read from
//! `~/.cinch/config.json`. Keychain-backed key reads will land alongside
//! the Rust credstore in Phase 6+.

use std::io::Read;
use std::sync::Arc;
use std::time::Instant;

use client_core::auth::load_config;
use client_core::auth_session;
use client_core::config::Config;
use client_core::crypto;
use client_core::http::RestClient;
use client_core::machine::hostname_or_unknown;
use client_core::rest::{ContentType, PushRequest};
use client_core::store::{self, Store};
use client_core::sync::{LocalPusher, PushOutcome};

use crate::exit::{ExitError, AUTH_FAILURE, ENCRYPTION_REQUIRED, GENERIC_ERROR};

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

    /// Override auth token (or set CINCH_TOKEN env var).
    #[arg(long)]
    pub token: Option<String>,

    /// Override relay URL (or set CINCH_RELAY_URL env var).
    #[arg(long)]
    pub relay: Option<String>,

    /// Send only to the device with this nickname or hostname (resolved
    /// via GET /devices to a target_device_id). The relay rejects the
    /// push with `device_offline` if the resolved device is not currently
    /// connected.
    #[arg(long)]
    pub to: Option<String>,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    // Note: no `ensure_authenticated()` guard here. `resolve_config` below
    // overlays `--token` and `CINCH_TOKEN` on top of disk state and then
    // emits the same `AUTH_FAILURE` + `Run: cinch auth login` error when
    // every source is empty, so adding the guard would override the
    // documented stateless-push path (CI / containers without `~/.cinch`).
    let cfg = resolve_config(&args)?;

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

    // T4: piggyback a brief backlog flush so any pending clips arrive
    // at the relay before this one. Debounced to once per 60s via the
    // persistent `last_flush_at` watermark and bounded to 250 ms so the
    // user-visible push latency stays predictable. Gated by the
    // process-static `SESSION_FLUSH_GATE` so it skips silently when T1
    // (main.rs `spawn_session_flush`) is already mid-flight — the
    // relay's `client_created_at` clamp preserves chronological order
    // at the receiver in the rare contended-race case.
    if let Ok(ctx) = crate::runtime::open_ctx() {
        crate::runtime::try_session_flush_with_timeout(&ctx, std::time::Duration::from_millis(250))
            .await;
    }

    let hostname = hostname_or_unknown();
    let source = format!("remote:{}", hostname);

    let detected = detect_content_type(&data);
    let is_binary = if args.text {
        false
    } else if let Some(ft) = &args.force_type {
        force_is_image(ft)
    } else {
        matches!(detected, ContentType::Image)
    };
    let wire_type = if is_binary {
        ContentType::Image
    } else if args.text {
        // `--text` is an explicit user request to treat the input as plain
        // text — bypass classification so url/code-shaped input doesn't
        // surprise the caller.
        ContentType::Text
    } else {
        // Classify the text payload into Text / Url / Code so the
        // wire and local store carry a canonical, downstream-meaningful
        // type instead of an opaque "text". `classify::detect` takes
        // bytes directly — for 20 MB pushes this avoids an upfront
        // O(n) UTF-8 walk that the classifier would short-circuit anyway.
        client_core::classify::detect(&data)
    };

    let client = RestClient::new(
        cfg.relay_url.clone(),
        cfg.token.clone(),
        crate::client_info::for_cli(),
    )
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not init client: {}", e), ""))?;

    // E2EE-001: if the device is still waiting on a paired peer to share
    // the master AES key, attempt one inline key-exchange round before
    // failing. Without this, the user would have to manually run
    // `cinch auth retry-key` between every fresh-login and first push.
    crate::key_state::ensure_master_key(&cfg, &client).await?;

    let target_device_id = match args.to.as_deref().filter(|s| !s.is_empty()) {
        Some(name) => Some(resolve_target_device_id(&client, name).await?),
        None => None,
    };

    let start = Instant::now();
    let original_size = data.len() as i64;

    // Targeted pushes (`--to <device>`) keep the bespoke flow because
    // `LocalPusher::push_text` does not carry `target_device_id` today.
    if let Some(target) = target_device_id.as_deref() {
        let data_for_store = data.clone();
        let resp = if is_binary {
            push_binary(
                &client,
                &cfg,
                data,
                &source,
                args.label.as_deref(),
                Some(target),
            )
            .await?
        } else {
            push_text(
                &client,
                &cfg,
                data,
                &source,
                args.label.as_deref(),
                wire_type,
                Some(target),
            )
            .await?
        };

        // Best-effort local write-through. We never fail the command on a store
        // error — the relay already accepted the push, and the next backfill
        // reconciles. `synced=true` because we only reach this branch after
        // the relay returned 200.
        if let Ok(ctx) = crate::runtime::open_ctx() {
            let content_type = wire_type.as_wire().to_string();
            let created_at = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            let stored = client_core::store::models::StoredClip {
                id: resp.clip_id.clone(),
                source: source.clone(),
                source_key: None,
                content_type,
                content: Some(data_for_store),
                media_path: None,
                byte_size: resp.byte_size,
                created_at,
                pinned: false,
                pinned_at: None,
                sync_state: client_core::store::models::SyncState::Synced,
            };
            let _ = client_core::store::queries::insert_clip(&ctx.store, &stored);
            let _ = client_core::store::queries::set_watermark(&ctx.store, &resp.clip_id);
        }

        if !args.silent {
            eprintln!(
                "\u{2713} Pushed {} \u{00B7} {} ms",
                format_bytes(resp.byte_size),
                start.elapsed().as_millis()
            );
        }
        return Ok(());
    }

    // Untargeted path — route through `LocalPusher` so transient relay errors
    // (or a missing enc_key) enqueue the clip locally with `synced=false`.
    // A future flush picks it up via `client_core::sync::flush_once`.
    //
    // If we cannot open the local store (rare — typically `--token`/`--relay`
    // invocations against an unauthenticated home dir), fall back to the
    // bespoke flow so we don't refuse to push.
    let pusher = open_local_pusher(&cfg, client.clone());

    if let Some(pusher) = pusher {
        let outcome = if is_binary {
            pusher
                .push_image_png(data, &source, args.label.as_deref().unwrap_or(""))
                .await
                .map_err(map_ingest_error)?
        } else {
            pusher
                .push_text(
                    data,
                    &source,
                    args.label.as_deref().unwrap_or(""),
                    wire_type,
                )
                .await
                .map_err(map_ingest_error)?
        };

        match outcome {
            PushOutcome::Synced(_clip_id) => {
                if !args.silent {
                    eprintln!(
                        "\u{2713} Pushed {} \u{00B7} {} ms",
                        format_bytes(original_size),
                        start.elapsed().as_millis()
                    );
                }
            }
            PushOutcome::Queued(clip_id) => {
                if !args.silent {
                    eprintln!(
                        "\u{21BB} Queued {} offline (id={}) \u{00B7} {} ms",
                        format_bytes(original_size),
                        clip_id,
                        start.elapsed().as_millis()
                    );
                }
            }
        }
        return Ok(());
    }

    // Fallback bespoke flow when no local store is available. Without a
    // store there is no write-through and no enqueue — the relay push is
    // the only durable hop.
    let resp = if is_binary {
        push_binary(&client, &cfg, data, &source, args.label.as_deref(), None).await?
    } else {
        push_text(
            &client,
            &cfg,
            data,
            &source,
            args.label.as_deref(),
            wire_type,
            None,
        )
        .await?
    };

    if !args.silent {
        eprintln!(
            "\u{2713} Pushed {} \u{00B7} {} ms",
            format_bytes(resp.byte_size),
            start.elapsed().as_millis()
        );
    }
    Ok(())
}

/// Construct a `LocalPusher` from the resolved CLI config. Returns `None`
/// when the user is unauthenticated, the encryption key is missing, or
/// the local store cannot be opened — the caller falls back to the
/// bespoke push flow in that case, which surfaces the existing
/// `AUTH_FAILURE` / `ENCRYPTION_REQUIRED` exit codes via `require_key`
/// so `cinch push` against an unauthenticated home dir errors out
/// rather than silently queueing.
fn open_local_pusher(cfg: &Config, client: RestClient) -> Option<LocalPusher> {
    if cfg.user_id.is_empty() {
        return None;
    }
    let enc_key = auth_session::require_encryption_key(&cfg.user_id).ok()?;
    let store_path = store::default_db_path().ok()?;
    let store = Store::open(&store_path).ok()?;
    Some(LocalPusher::new(
        Arc::new(store),
        Arc::new(client),
        Some(enc_key),
    ))
}

/// Translate `client_core::sync::IngestError` into the CLI's exit-coded
/// error type. Relay-side `Push` failures are already wrapped by
/// `HttpError`, which has its own `From` impl via `crate::exit`.
fn map_ingest_error(err: client_core::sync::IngestError) -> ExitError {
    use client_core::sync::IngestError;
    match err {
        // Defensive: `open_local_pusher` now refuses to construct a
        // `LocalPusher` without an encryption key, so this arm is
        // unreachable in practice. Kept as a safe fallback.
        IngestError::NoEncryptionKey => ExitError::new(
            ENCRYPTION_REQUIRED,
            "Encryption key missing. End-to-end encryption is required.",
            "Run: cinch auth login (regenerates and stores your key).",
        ),
        IngestError::Crypto(msg) => ExitError::new(
            ENCRYPTION_REQUIRED,
            format!("Encryption failed: {}", msg),
            "Re-run: cinch auth login",
        ),
        // Defensive: `LocalPusher::push_text`/`push_image_png` swallow
        // transient `Push` errors and convert them to `Queued`, so this
        // arm should not fire today. Kept for forward compatibility.
        IngestError::Push(http_err) => ExitError::from(http_err),
        IngestError::Store(msg) => ExitError::new(
            GENERIC_ERROR,
            format!("Local store write failed: {}", msg),
            "",
        ),
        // Surfaced by `LocalPusher::send_stored` when the requested clip id is
        // unknown or has no sendable plaintext (e.g. media-only).
        IngestError::NotFound(id) => ExitError::new(
            GENERIC_ERROR,
            format!("Clip not found or has no sendable content: {}", id),
            "",
        ),
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

fn require_key(cfg: &Config) -> Result<[u8; 32], ExitError> {
    if cfg.user_id.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "Not authenticated.",
            "Run: cinch auth login",
        ));
    }
    auth_session::require_encryption_key(&cfg.user_id).map_err(crate::key_state::classify_key_error)
}

async fn push_text(
    client: &RestClient,
    cfg: &Config,
    data: Vec<u8>,
    source: &str,
    label: Option<&str>,
    content_type: ContentType,
    target_device_id: Option<&str>,
) -> Result<client_core::rest::PushResponse, ExitError> {
    let original_size = data.len() as i64;
    let key = require_key(cfg)?;
    let content = crypto::encrypt(&key, &data).map_err(|e| {
        ExitError::new(
            ENCRYPTION_REQUIRED,
            format!("Encryption failed: {}", e),
            "Re-run: cinch auth login",
        )
    })?;
    let req = PushRequest {
        content,
        content_type: content_type.as_wire().to_string(),
        label: label.unwrap_or("").to_string(),
        source: source.to_string(),
        media_path: None,
        byte_size: original_size,
        encrypted: true,
        target_device_id: target_device_id.map(|s| s.to_string()),
        client_created_at: None,
        idempotency_key: None,
    };
    Ok(client.push_clip_json(&req).await?)
}

async fn push_binary(
    client: &RestClient,
    cfg: &Config,
    data: Vec<u8>,
    source: &str,
    label: Option<&str>,
    target_device_id: Option<&str>,
) -> Result<client_core::rest::PushResponse, ExitError> {
    let key = require_key(cfg)?;
    let ciphertext = crypto::encrypt(&key, &data).map_err(|e| {
        ExitError::new(
            ENCRYPTION_REQUIRED,
            format!("Encryption failed: {}", e),
            "Re-run: cinch auth login",
        )
    })?;
    let req = PushRequest {
        content: ciphertext,
        content_type: ContentType::Image.as_wire().to_string(),
        label: label.unwrap_or("").to_string(),
        source: source.to_string(),
        media_path: None,
        byte_size: data.len() as i64,
        encrypted: true,
        target_device_id: target_device_id.map(|s| s.to_string()),
        client_created_at: None,
        idempotency_key: None,
    };
    Ok(client.push_clip_json(&req).await?)
}

/// Resolve a user-supplied `--to <name>` to a target device_id.
///
/// Matches `name` case-insensitively against each device's nickname, then
/// hostname. If neither matches, returns a `GENERIC_ERROR` with the list
/// of known names so the user can correct the typo without round-tripping
/// to `cinch device list`.
async fn resolve_target_device_id(client: &RestClient, name: &str) -> Result<String, ExitError> {
    let devices = client.list_devices().await.map_err(|e| {
        ExitError::new(
            GENERIC_ERROR,
            format!("Could not list devices: {}", e),
            "Check connectivity and retry; or omit --to.",
        )
    })?;
    let lower = name.to_lowercase();
    for d in &devices {
        let nick_match = !d.nickname.is_empty() && d.nickname.to_lowercase() == lower;
        let host_match = d.hostname.to_lowercase() == lower;
        if nick_match || host_match {
            return Ok(d.id.clone());
        }
    }
    let known: Vec<String> = devices
        .iter()
        .map(|d| {
            if d.nickname.is_empty() {
                d.hostname.clone()
            } else {
                format!("{} ({})", d.nickname, d.hostname)
            }
        })
        .collect();
    let hint = if known.is_empty() {
        "No devices paired yet. Run: cinch auth login on the target machine.".to_string()
    } else {
        format!("Known devices: {}", known.join(", "))
    };
    Err(ExitError::new(
        GENERIC_ERROR,
        format!("No device matches '{}'.", name),
        hint,
    ))
}

/// `--type` accepts either canonical `image` or any `image/*` MIME for
/// backwards compatibility with prior CLI invocations.
fn force_is_image(s: &str) -> bool {
    s == "image" || s.starts_with("image/")
}

/// Sniffs image magic bytes; falls back to Text. The Text return is then
/// refined into Text / Url / Code by `client_core::classify::detect`.
fn detect_content_type(data: &[u8]) -> ContentType {
    let is_image = data.starts_with(b"\x89PNG\r\n\x1a\n")
        || data.starts_with(b"\xff\xd8\xff")
        || data.starts_with(b"GIF87a")
        || data.starts_with(b"GIF89a")
        || (data.starts_with(b"RIFF") && data.len() >= 12 && &data[8..12] == b"WEBP");
    if is_image {
        ContentType::Image
    } else {
        ContentType::Text
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

#[cfg(test)]
mod tests {
    use super::*;
    use client_core::store::StoreError;
    use client_core::sync::IngestError;

    fn args_with(token: Option<&str>, relay: Option<&str>) -> Args {
        Args {
            label: None,
            silent: false,
            force_type: None,
            text: false,
            token: token.map(String::from),
            relay: relay.map(String::from),
            to: None,
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
        assert!(matches!(detect_content_type(png), ContentType::Image));
    }

    #[test]
    fn detect_content_type_recognizes_jpeg() {
        let jpeg = b"\xff\xd8\xff\xe0\x00\x10JFIF";
        assert!(matches!(detect_content_type(jpeg), ContentType::Image));
    }

    #[test]
    fn detect_content_type_recognizes_gif87a_and_gif89a() {
        assert!(matches!(detect_content_type(b"GIF87a"), ContentType::Image));
        assert!(matches!(detect_content_type(b"GIF89a"), ContentType::Image));
    }

    #[test]
    fn detect_content_type_recognizes_webp() {
        // RIFF<size>WEBP — the `WEBP` marker at bytes 8..12 is load-bearing.
        let webp = b"RIFF\x24\x00\x00\x00WEBPVP8 ";
        assert!(matches!(detect_content_type(webp), ContentType::Image));
    }

    #[test]
    fn detect_content_type_text_fallback() {
        assert!(matches!(detect_content_type(b"hello"), ContentType::Text));
        assert!(matches!(detect_content_type(b""), ContentType::Text));
        // RIFF without the WEBP marker (e.g. AVI) must NOT be classified as image.
        assert!(matches!(
            detect_content_type(b"RIFF\0\0\0\0AVI LIST"),
            ContentType::Text
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

    // --- map_ingest_error ---------------------------------------------------

    #[test]
    fn map_ingest_error_no_key_signals_encryption_required() {
        let exit = map_ingest_error(IngestError::NoEncryptionKey);
        assert_eq!(exit.code, ENCRYPTION_REQUIRED);
    }

    #[test]
    fn map_ingest_error_crypto_keeps_underlying_message() {
        let exit = map_ingest_error(IngestError::Crypto("bad nonce".into()));
        assert_eq!(exit.code, ENCRYPTION_REQUIRED);
        // The underlying message must surface — silently swallowing it
        // would make encryption bugs impossible to triage from `cinch push`.
        assert!(
            exit.message.contains("bad nonce"),
            "expected message to surface underlying error, got: {}",
            exit.message
        );
    }

    #[test]
    fn map_ingest_error_store_returns_generic_with_message() {
        let exit = map_ingest_error(IngestError::Store(StoreError::Migration(
            "disk full".into(),
        )));
        assert_eq!(exit.code, GENERIC_ERROR);
        assert!(
            exit.message.contains("disk full"),
            "expected message to surface store error, got: {}",
            exit.message
        );
    }

    // --- resolve_config -----------------------------------------------------

    #[test]
    fn resolve_config_with_both_flags_short_circuits_disk() {
        // When --token AND --relay are both passed, resolve_config returns
        // without touching ~/.cinch/config.json. Verify the early-return
        // path AND that the relay URL's trailing slash is stripped (the
        // relay's path matcher is strict about double slashes).
        let args = args_with(Some("tok-xyz"), Some("https://relay.example/"));
        let cfg = resolve_config(&args).expect("args short-circuit succeeds");
        assert_eq!(cfg.token, "tok-xyz");
        assert_eq!(cfg.relay_url, "https://relay.example");
        // The args-only branch uses Config::default() for everything else,
        // so user_id stays empty — `require_key` will then fail cleanly
        // with AUTH_FAILURE downstream.
        assert!(cfg.user_id.is_empty());
    }
}
