//! `cinch pull` — read clipboard content from the relay.
//!
//! Without `--from`: hits `GET /clips/latest` with no params to fetch the
//! absolute most recent clip across every paired device.
//! With `--from`:    hits `GET /clips/latest?source=remote:<resolved>` and
//! optionally fetches the image two-step via `GET /clips/{id}/media`.

use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

use arboard::Clipboard;
use client_core::auth::load_config;
use client_core::config::Config;
use client_core::credstore;
use client_core::crypto;
use client_core::http::RestClient;
use client_core::protocol::Clip;
use client_core::rest::ContentType;
use client_core::ws::{self, DecryptFailReason, WsConfig, WsEvent, WsStatus};

/// Debounce guard: allows at most one `retry_key_bundle` call per window.
struct RetryGate {
    last: Option<Instant>,
    debounce: Duration,
}
impl RetryGate {
    fn new() -> Self {
        Self {
            last: None,
            debounce: Duration::from_secs(60),
        }
    }
    fn try_take(&mut self) -> bool {
        let now = Instant::now();
        match self.last {
            Some(t) if now.duration_since(t) < self.debounce => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

use crate::exit::{ExitError, AUTH_FAILURE, ENCRYPTION_PENDING, GENERIC_ERROR, RELAY_ERROR};

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Pull latest clip from a device by nickname or hostname.
    #[arg(long)]
    pub from: Option<String>,

    /// Fetch a specific clip by ID instead of "latest".
    #[arg(long, conflicts_with_all = ["from", "watch", "exclude_self"])]
    pub id: Option<String>,

    /// Print to stdout only; do not write to the system clipboard.
    #[arg(long)]
    pub raw: bool,

    /// Skip image clips, return latest text clip.
    #[arg(long = "text-only")]
    pub text_only: bool,

    /// Copy text content to system clipboard (TTY only). Ignored when --raw is set.
    #[arg(long)]
    pub copy: bool,

    /// Exclude clips authored by the local device. Incompatible with --from and --id.
    #[arg(long = "exclude-self", conflicts_with_all = ["from", "id"])]
    pub exclude_self: bool,

    /// Subscribe to live clip stream over WebSocket and print each clip
    /// to stdout as it arrives (one clip per line, prefixed with source).
    /// Combine with `--from <name>` to filter to a single source.
    #[arg(long, conflicts_with_all = ["id", "exclude_self"])]
    pub watch: bool,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    let cfg = load_config().map_err(|e| {
        ExitError::new(
            AUTH_FAILURE,
            format!("Could not load config: {}", e),
            "Run: cinch auth login",
        )
    })?;
    if cfg.token.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "No auth token configured.",
            "Run: cinch auth login",
        ));
    }
    let client = RestClient::new(
        cfg.relay_url.clone(),
        cfg.token.clone(),
        crate::client_info::for_cli(),
    )
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not init client: {}", e), ""))?;

    // E2EE-001: ensure we have a master AES key (with a single inline
    // key-exchange retry on PendingExchange) before any decrypt path can
    // surface aead::Error to the user.
    crate::key_state::ensure_master_key(&cfg, &client).await?;

    // Make sure the local store is fresh before any read path that may
    // fall back to it (no caller currently does, but this keeps the
    // store warm and reduces lag between push-on-A and pull-on-B).
    if let Ok(ctx) = crate::runtime::open_ctx() {
        crate::runtime::opportunistic_backfill(&ctx).await;
    }

    if let Some(clip_id) = args.id.as_deref() {
        let mut clip = client.get_clip_by_id(clip_id).await.map_err(|e| {
            ExitError::new(
                RELAY_ERROR,
                format!("Fetch clip {} failed: {}", clip_id, e),
                "",
            )
        })?;
        clip = decrypt_clip(&cfg, clip)?;

        // Image clip: emit raw bytes when --raw is set; otherwise mirror the existing image path.
        let is_image = matches!(content_type(&clip.content_type), Some(ContentType::Image));
        if is_image {
            let bytes = client.get_clip_media(clip_id).await.map_err(|e| {
                ExitError::new(
                    RELAY_ERROR,
                    format!("Fetch clip media {} failed: {}", clip_id, e),
                    "",
                )
            })?;
            if args.raw {
                use std::io::Write;
                std::io::stdout().write_all(&bytes).map_err(|e| {
                    ExitError::new(GENERIC_ERROR, format!("write image bytes: {}", e), "")
                })?;
                return Ok(());
            }
            return write_image(&clip, bytes, false, clip_id);
        }

        // Text clip.
        print!("{}", clip.content);
        let _ = std::io::stdout().flush();
        if !args.raw && args.copy && !clip.content.is_empty() {
            copy_text_to_clipboard(&clip.content);
        }
        return Ok(());
    }

    if args.exclude_self {
        let self_key = client_core::machine::self_source_key();
        let mut clip = client
            .get_latest_clip_excluding(&self_key)
            .await
            .map_err(|e| {
                ExitError::new(
                    RELAY_ERROR,
                    format!("Fetch latest (excluding self) failed: {}", e),
                    "",
                )
            })?;
        clip = decrypt_clip(&cfg, clip)?;

        let is_image = matches!(content_type(&clip.content_type), Some(ContentType::Image));
        if is_image {
            let bytes = client.get_clip_media(&clip.clip_id).await.map_err(|e| {
                ExitError::new(
                    RELAY_ERROR,
                    format!("Fetch clip media {} failed: {}", clip.clip_id, e),
                    "",
                )
            })?;
            if args.raw {
                use std::io::Write;
                std::io::stdout().write_all(&bytes).map_err(|e| {
                    ExitError::new(GENERIC_ERROR, format!("write image bytes: {}", e), "")
                })?;
                return Ok(());
            }
            return write_image(&clip, bytes, false, "any (excluding self)");
        }

        print!("{}", clip.content);
        let _ = std::io::stdout().flush();
        if !args.raw && args.copy && !clip.content.is_empty() {
            copy_text_to_clipboard(&clip.content);
        }
        return Ok(());
    }

    if args.watch {
        let from_filter = match args.from.as_deref() {
            Some(name) => Some(resolve_source(&client, name).await),
            None => None,
        };
        return watch_stream(&cfg, &client, from_filter, args.text_only, args.copy).await;
    }

    if let Some(from) = args.from {
        pull_from_source(&client, &cfg, &from, args.text_only, args.copy).await
    } else {
        pull_latest_any(&client, &cfg, args.text_only, args.copy).await
    }
}

async fn watch_stream(
    cfg: &Config,
    client: &RestClient,
    from_source: Option<String>,
    text_only: bool,
    copy: bool,
) -> Result<(), ExitError> {
    let key = credstore::read_encryption_key(&cfg.user_id);
    let (tx, mut rx) = tokio::sync::mpsc::channel::<WsEvent>(64);
    let cfg_ws = WsConfig {
        relay_url: cfg.relay_url.clone(),
        token: cfg.token.clone(),
        encryption_key: key,
        client_info: Some(crate::client_info::for_cli()),
    };
    let handle = tokio::spawn(ws::run(cfg_ws, tx));

    eprintln!("watching for clips… (Ctrl-C to stop)");
    let mut retry_gate = RetryGate::new();
    while let Some(event) = rx.recv().await {
        match event {
            WsEvent::Status(WsStatus::Connected) => eprintln!("connected"),
            WsEvent::Status(WsStatus::Disconnected) => eprintln!("disconnected — retrying…"),
            WsEvent::Status(WsStatus::Connecting) => {}
            WsEvent::NewClip { clip, plaintext } => {
                if let Some(filter) = &from_source {
                    if &clip.source != filter {
                        continue;
                    }
                }
                let is_image = matches!(content_type(&clip.content_type), Some(ContentType::Image));
                if text_only && is_image {
                    continue;
                }
                if is_image {
                    eprintln!(
                        "[{}, {}, from {}, id {}]",
                        clip.content_type,
                        format_bytes(clip.byte_size),
                        clip.source,
                        clip.clip_id
                    );
                    let mut out = std::io::stdout().lock();
                    let _ = out.write_all(&plaintext);
                    let _ = out.flush();
                } else {
                    let stdout = std::io::stdout();
                    if stdout.is_terminal() {
                        eprintln!("[{}] {}", clip.source, clip.content);
                    } else {
                        let mut out = stdout.lock();
                        let _ = out.write_all(clip.content.as_bytes());
                        let _ = out.write_all(b"\n");
                        let _ = out.flush();
                    }
                    if copy && !clip.content.is_empty() {
                        copy_text_to_clipboard(&clip.content);
                    }
                }
            }
            WsEvent::ClipDeleted { .. } => {}
            WsEvent::Revoked { reason } => {
                handle.abort();
                return Err(ExitError::new(
                    AUTH_FAILURE,
                    format!(
                        "Device revoked: {}",
                        reason.unwrap_or_else(|| "no reason given".into())
                    ),
                    "Run: cinch auth login",
                ));
            }
            WsEvent::TokenRotated { token, device_id } => {
                if let (Ok(current_cfg), Some(did)) = (load_config(), device_id.as_deref()) {
                    match client_core::auth::rotate_credentials(
                        &current_cfg.user_id,
                        did,
                        &token,
                        &current_cfg.hostname,
                    ) {
                        Ok(()) => eprintln!("note: token rotated and persisted"),
                        Err(e) => eprintln!("note: token rotated but persist failed: {}", e),
                    }
                } else {
                    eprintln!("note: token rotated by relay; restart cinch pull --watch");
                }
            }
            WsEvent::ClipDecryptFailed { clip_id, reason } => {
                let reason_str = match &reason {
                    DecryptFailReason::MissingKey => {
                        "no encryption key — run: cinch auth retry-key".into()
                    }
                    DecryptFailReason::TagFailed(e) => format!("key mismatch ({})", e),
                };
                eprintln!("cinch: cannot decrypt clip {}: {}", clip_id, reason_str);
                if retry_gate.try_take() {
                    match client.retry_key_bundle().await {
                        Ok(_) => {
                            eprintln!("cinch: requested re-share of encryption key from peers")
                        }
                        Err(e) => eprintln!("cinch: retry_key_bundle failed: {}", e),
                    }
                }
            }
            WsEvent::KeyExchangeRequested { device_id } => {
                if let (Some(did), Some(key_bytes)) =
                    (device_id, credstore::read_encryption_key(&cfg.user_id))
                {
                    if let Err(e) =
                        client_core::key_exchange::handle_event(client, &did, &key_bytes).await
                    {
                        eprintln!("key exchange failed: {}", e);
                    }
                }
            }
        }
    }
    Ok(())
}

async fn pull_latest_any(
    client: &RestClient,
    cfg: &Config,
    text_only: bool,
    copy: bool,
) -> Result<(), ExitError> {
    let mut clip = client
        .get_latest_clip_any()
        .await
        .map_err(|e| ExitError::new(RELAY_ERROR, format!("Fetch latest clip failed: {}", e), ""))?;
    if clip.encrypted {
        clip = decrypt_clip(cfg, clip)?;
    }

    let is_image = matches!(content_type(&clip.content_type), Some(ContentType::Image));

    if text_only && is_image {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "Latest clip is an image.",
            "Drop --text-only to pull it, or use --from <device>.",
        ));
    }

    if is_image {
        let bytes = if clip
            .media_path
            .as_deref()
            .filter(|s| !s.is_empty())
            .is_some()
        {
            client.get_clip_media(&clip.clip_id).await.map_err(|e| {
                ExitError::new(
                    RELAY_ERROR,
                    format!("Fetch clip media {} failed: {}", clip.clip_id, e),
                    "",
                )
            })?
        } else {
            clip.content.clone().into_bytes()
        };
        return write_image(&clip, bytes, copy, "any");
    }

    print!("{}", clip.content);
    if copy && !clip.content.is_empty() {
        copy_text_to_clipboard(&clip.content);
    }
    let _ = std::io::stdout().flush();
    Ok(())
}

async fn pull_from_source(
    client: &RestClient,
    cfg: &Config,
    from: &str,
    text_only: bool,
    copy: bool,
) -> Result<(), ExitError> {
    let source = resolve_source(client, from).await;
    let mut clip = client.get_latest_clip(&source).await?;

    if clip.encrypted {
        clip = decrypt_clip(cfg, clip)?;
    }

    let is_image = matches!(content_type(&clip.content_type), Some(ContentType::Image));

    if text_only && is_image {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "Latest clip is an image.",
            "Use --from without --text-only to pull it.",
        ));
    }

    // Image clips when ENCRYPTED have plaintext bytes already in clip.content
    // (decrypt_clip wrote them there via from_utf8_lossy fallback). For
    // unencrypted image clips, fetch via /clips/{id}/media.
    if is_image {
        let bytes = if clip
            .media_path
            .as_deref()
            .filter(|s| !s.is_empty())
            .is_some()
        {
            client.get_clip_media(&clip.clip_id).await?
        } else {
            // Encrypted image content was decoded into clip.content as raw
            // bytes — but JSON cannot carry binary cleanly, so the encrypted
            // path stored content as raw plaintext and we recover it as
            // bytes from the underlying String.
            clip.content.clone().into_bytes()
        };
        return write_image(&clip, bytes, copy, from);
    }

    // Text clip.
    print!("{}", clip.content);
    if copy && !clip.content.is_empty() {
        copy_text_to_clipboard(&clip.content);
    }
    let _ = std::io::stdout().flush();
    Ok(())
}

async fn resolve_source(client: &RestClient, from: &str) -> String {
    let default = format!("remote:{}", from);
    let Ok(devices) = client.list_devices().await else {
        return default;
    };
    let lower = from.to_lowercase();
    for d in devices {
        let nick_match = !d.nickname.is_empty() && d.nickname.to_lowercase() == lower;
        let host_match = d.hostname.to_lowercase() == lower;
        if nick_match || host_match {
            return d.source_key;
        }
    }
    default
}

fn decrypt_clip(cfg: &Config, mut clip: Clip) -> Result<Clip, ExitError> {
    let key = credstore::read_encryption_key(&cfg.user_id).ok_or_else(|| {
        // No key on disk: distinguish "pending ECDH" from "not signed in"
        // by consulting cfg.key_pending. Surfacing PendingExchange points
        // the user at `cinch auth retry-key` instead of a destructive
        // `cinch auth login`.
        let err = if cfg.key_pending {
            client_core::auth_session::RequireKeyError::PendingExchange
        } else {
            client_core::auth_session::RequireKeyError::Missing
        };
        crate::key_state::classify_key_error(err)
    })?;
    let plaintext = crypto::decrypt(&key, &clip.content).map_err(|e| {
        // We have a key, but it can't decrypt this clip — the key is most
        // likely a stale one persisted before the device joined a paired
        // session. Point users at retry-key rather than login so they
        // don't wipe their working credentials.
        ExitError::new(
            ENCRYPTION_PENDING,
            format!("Decryption failed: {}", e),
            "Encryption key is out of sync with the sender. Try: cinch auth retry-key",
        )
    })?;
    if matches!(content_type(&clip.content_type), Some(ContentType::Image)) {
        // Stash raw image bytes back into the String. Callers in the image
        // branch reach for `clip.content.into_bytes()` and recover them.
        clip.content = unsafe { String::from_utf8_unchecked(plaintext) };
    } else {
        clip.content = String::from_utf8(plaintext).map_err(|e| {
            ExitError::new(
                GENERIC_ERROR,
                format!("Decrypted content is not valid UTF-8: {}", e),
                "",
            )
        })?;
    }
    Ok(clip)
}

fn content_type(s: &str) -> Option<ContentType> {
    match s {
        "text" => Some(ContentType::Text),
        "url" => Some(ContentType::Url),
        "code" => Some(ContentType::Code),
        "image" => Some(ContentType::Image),
        _ => None,
    }
}

fn copy_text_to_clipboard(text: &str) {
    if let Ok(mut cb) = Clipboard::new() {
        if let Err(e) = cb.set_text(text) {
            eprintln!("Warning: clipboard write failed: {}", e);
        }
    } else {
        eprintln!("Warning: could not open system clipboard");
    }
}

fn write_image(clip: &Clip, bytes: Vec<u8>, copy: bool, from: &str) -> Result<(), ExitError> {
    let stdout = std::io::stdout();
    if stdout.is_terminal() {
        eprintln!(
            "[{}, {}, from {}]",
            clip.content_type,
            format_bytes(clip.byte_size),
            clip.source
        );
        if copy {
            #[cfg(target_os = "macos")]
            {
                match crate::macos_pasteboard::write_png(&bytes) {
                    Ok(()) => eprintln!("  Copied image to clipboard."),
                    Err(e) => eprintln!("  Warning: clipboard write failed: {}", e),
                }
                return Ok(());
            }
            // Non-macOS: arboard's image API requires PNG → RGBA decode, which
            // pulls in the `image` crate. Until we want that footprint, point
            // the user at the pipe-to-file workaround.
            #[cfg(not(target_os = "macos"))]
            {
                eprintln!("  --copy for images is only supported on macOS.");
                eprintln!(
                    "  Pipe to a file instead: cinch pull --from {} > image.png",
                    from
                );
                return Ok(());
            }
        }
        eprintln!("  Use --copy, or pipe to a file:");
        eprintln!("  cinch pull --from {} > image.png", from);
        return Ok(());
    }
    let mut out = stdout.lock();
    out.write_all(&bytes)
        .map_err(|e| ExitError::new(RELAY_ERROR, format!("write stdout: {}", e), ""))?;
    Ok(())
}

fn format_bytes(n: i64) -> String {
    let n = n as f64;
    if n >= 1024.0 * 1024.0 {
        format!("{:.1}MB", n / (1024.0 * 1024.0))
    } else if n >= 1024.0 {
        format!("{:.1}KB", n / 1024.0)
    } else {
        format!("{} bytes", n as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Debug, Parser)]
    #[command(no_binary_name = true)]
    struct PullArgsHarness {
        #[command(flatten)]
        args: Args,
    }

    #[test]
    fn args_id_with_from_is_rejected() {
        let err =
            PullArgsHarness::try_parse_from(["--id", "abc", "--from", "desktop"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn args_id_with_watch_is_rejected() {
        let err = PullArgsHarness::try_parse_from(["--id", "abc", "--watch"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn args_exclude_self_with_from_is_rejected() {
        let err =
            PullArgsHarness::try_parse_from(["--exclude-self", "--from", "desktop"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn args_id_with_exclude_self_is_rejected() {
        let err = PullArgsHarness::try_parse_from(["--id", "abc", "--exclude-self"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn args_watch_with_exclude_self_is_rejected() {
        let err = PullArgsHarness::try_parse_from(["--watch", "--exclude-self"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn args_watch_with_from_is_allowed() {
        let harness =
            PullArgsHarness::try_parse_from(["--watch", "--from", "desktop"]).expect("parse ok");
        assert!(harness.args.watch);
        assert_eq!(harness.args.from.as_deref(), Some("desktop"));
    }

    #[test]
    fn content_type_maps_canonical_strings() {
        assert!(matches!(content_type("text"), Some(ContentType::Text)));
        assert!(matches!(content_type("url"), Some(ContentType::Url)));
        assert!(matches!(content_type("code"), Some(ContentType::Code)));
        assert!(matches!(content_type("image"), Some(ContentType::Image)));
    }

    #[test]
    fn content_type_rejects_unknown_strings() {
        // Mime styles and case variants must not slip through — wire is strict.
        assert!(content_type("").is_none());
        assert!(content_type("Text").is_none());
        assert!(content_type("TEXT").is_none());
        assert!(content_type("text/plain").is_none());
        assert!(content_type("image/png").is_none());
        assert!(content_type("json").is_none());
    }

    #[test]
    fn format_bytes_under_kilobyte_is_raw_bytes() {
        assert_eq!(format_bytes(0), "0 bytes");
        assert_eq!(format_bytes(1), "1 bytes");
        assert_eq!(format_bytes(1023), "1023 bytes");
    }

    #[test]
    fn format_bytes_kilobyte_range() {
        assert_eq!(format_bytes(1024), "1.0KB");
        assert_eq!(format_bytes(1536), "1.5KB");
        assert_eq!(format_bytes(1024 * 1024 - 1), "1024.0KB");
    }

    #[test]
    fn format_bytes_megabyte_range() {
        assert_eq!(format_bytes(1024 * 1024), "1.0MB");
        assert_eq!(format_bytes(1024 * 1024 * 5 / 2), "2.5MB");
        assert_eq!(format_bytes(1024 * 1024 * 100), "100.0MB");
    }

    #[test]
    fn retry_gate_first_take_succeeds() {
        let mut gate = RetryGate::new();
        assert!(gate.try_take());
    }

    #[test]
    fn retry_gate_second_take_within_debounce_is_blocked() {
        let mut gate = RetryGate::new();
        assert!(gate.try_take());
        // The debounce is 60s in production; back-to-back calls must be blocked.
        assert!(!gate.try_take());
        assert!(!gate.try_take());
    }

    #[test]
    fn retry_gate_zero_debounce_always_allows() {
        // Construct manually to verify the elapsed-vs-debounce comparison
        // honors a sub-resolution window. Anything shorter than the time
        // between two consecutive `Instant::now()` calls must pass.
        let mut gate = RetryGate {
            last: None,
            debounce: Duration::from_nanos(0),
        };
        assert!(gate.try_take());
        assert!(gate.try_take());
    }
}
