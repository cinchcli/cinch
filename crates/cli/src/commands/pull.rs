//! `cinch pull` — read clipboard content from the relay.
//!
//! Without `--from`: hits `GET /clips/latest` with no params to fetch the
//! absolute most recent clip across every paired device.
//! With `--from`:    hits `GET /clips/latest?source=remote:<resolved>` and
//! optionally fetches the image two-step via `GET /clips/{id}/media`.

use std::io::{IsTerminal, Write};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use client_core::auth::load_config;
use client_core::config::Config;
use client_core::credstore;
use client_core::crypto;
use client_core::http::RestClient;
use client_core::protocol::Clip;
use client_core::sync::map::is_binary;
use client_core::transport::{StreamTransport, WsStreamTransport};
use client_core::ws::{DecryptFailReason, WsConfig, WsEvent, WsStatus};

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
use crate::io::copy_text_to_clipboard;

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
    crate::auth_state::ensure_authenticated()?;
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

    if args.watch {
        let from_filter = match args.from.as_deref() {
            Some(name) => Some(resolve_source(&client, name).await),
            None => None,
        };
        return watch_stream(
            &cfg,
            &client,
            from_filter,
            args.text_only,
            args.copy && !args.raw,
        )
        .await;
    }

    let (clip, from) = if let Some(id) = args.id.as_deref() {
        (client.get_clip_by_id(id).await?, id.to_string())
    } else if args.exclude_self {
        let source = client_core::machine::self_source_key();
        (
            client.get_latest_clip_excluding(&source).await?,
            "any (excluding self)".into(),
        )
    } else if let Some(from) = args.from.as_deref() {
        let source = resolve_source(&client, from).await;
        (client.get_latest_clip(&source).await?, from.to_string())
    } else {
        (client.get_latest_clip_any().await?, "any".into())
    };

    let is_image = is_binary(&clip);
    if args.text_only && is_image {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "Latest clip is an image.",
            "Drop --text-only to pull it, or select a text clip.",
        ));
    }
    let key = client_core::auth_session::require_encryption_key(&cfg.user_id)
        .map_err(crate::key_state::classify_key_error)?;
    let bytes = read_clip_bytes(&client, &clip, &key).await?;
    if is_image {
        if args.raw {
            std::io::stdout().write_all(&bytes).map_err(|e| {
                ExitError::new(GENERIC_ERROR, format!("write image bytes: {}", e), "")
            })?;
            return Ok(());
        }
        return write_image(&clip, bytes, args.copy, &from);
    }
    let text = String::from_utf8(bytes).map_err(|e| {
        ExitError::new(
            GENERIC_ERROR,
            format!("Decrypted content is not valid UTF-8: {}", e),
            "",
        )
    })?;
    print!("{}", text);
    let _ = std::io::stdout().flush();
    if args.copy && !args.raw && !text.is_empty() {
        copy_text_to_clipboard(&text);
    }
    Ok(())
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
        // The stream transport fetches GET /clips/{id}/media for media-routed
        // clips (D-routing) through the `ClipTransport` seam. Cheap to clone
        // (reqwest::Client is Arc-backed internally).
        media_fetcher: Some(std::sync::Arc::new(client.clone())),
    };
    // Drive the relay WebSocket through the `StreamTransport` seam (the same
    // one the sync `Writer` uses), so `crate::ws::run` has a single production
    // entry point and stays `pub(crate)`.
    let handle = tokio::spawn(async move { WsStreamTransport.run_stream(cfg_ws, tx).await });

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
                let is_image = is_binary(&clip);
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
                    // The relay sends token_rotated only over the connection it
                    // authenticated, so `did` should be THIS device. Persisting a
                    // token under a different device's account key would write a
                    // stray credential and leave this device stuck on its stale
                    // token, so we refuse a mismatch. An empty local id (config
                    // predates the field) is treated as "unknown" and allowed
                    // through to preserve existing behavior.
                    if !current_cfg.active_device_id.is_empty()
                        && current_cfg.active_device_id != did
                    {
                        eprintln!(
                            "note: ignoring token_rotated for device {} (this device is {})",
                            did, current_cfg.active_device_id
                        );
                    } else {
                        match client_core::auth::rotate_credentials(
                            &current_cfg.user_id,
                            did,
                            &token,
                            &current_cfg.hostname,
                        ) {
                            Ok(()) => eprintln!("note: token rotated and persisted"),
                            Err(e) => eprintln!("note: token rotated but persist failed: {}", e),
                        }
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

async fn read_clip_bytes(
    client: &RestClient,
    clip: &Clip,
    key: &[u8; 32],
) -> Result<Vec<u8>, ExitError> {
    let bytes = if clip.content.is_empty()
        && clip
            .media_path
            .as_deref()
            .is_some_and(|path| !path.is_empty())
    {
        client.get_clip_media(&clip.clip_id).await?
    } else {
        clip.content.as_bytes().to_vec()
    };
    if !clip.encrypted {
        if is_binary(clip) && !clip.content.is_empty() {
            return STANDARD.decode(&bytes).map_err(|e| {
                ExitError::new(GENERIC_ERROR, format!("Invalid image encoding: {}", e), "")
            });
        }
        return Ok(bytes);
    }
    let ciphertext = std::str::from_utf8(&bytes).map_err(|e| {
        ExitError::new(
            GENERIC_ERROR,
            format!("Invalid encrypted clip encoding: {}", e),
            "",
        )
    })?;
    crypto::decrypt(key, ciphertext).map_err(|e| {
        ExitError::new(
            ENCRYPTION_PENDING,
            format!("Decryption failed: {}", e),
            "Encryption key is out of sync with the sender. Try: cinch auth retry-key",
        )
    })
}

async fn resolve_source(client: &RestClient, from: &str) -> String {
    let Ok(devices) = client.list_devices().await else {
        return format!("remote:{}", from);
    };
    crate::commands::shared::match_device_source(&devices, from)
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

    #[tokio::test]
    async fn image_bytes_survive_inline_and_media_routing() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let client =
            RestClient::new(server.uri(), "test-token", crate::client_info::for_cli()).unwrap();
        let key = [7u8; 32];
        let image = b"\x89PNG\r\n\x1a\n\xff";
        let ciphertext = crypto::encrypt(&key, image).unwrap();
        Mock::given(method("GET"))
            .and(path("/clips/routed/media"))
            .respond_with(ResponseTemplate::new(200).set_body_string(ciphertext.clone()))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/clips/plain/media"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(image.to_vec()))
            .expect(1)
            .mount(&server)
            .await;

        let inline = Clip {
            clip_id: "inline".into(),
            content_type: "image".into(),
            content: ciphertext.clone(),
            encrypted: true,
            ..Default::default()
        };
        assert_eq!(
            read_clip_bytes(&client, &inline, &key).await.unwrap(),
            image
        );
        let routed = Clip {
            clip_id: "routed".into(),
            content: String::new(),
            media_path: Some("media/routed.enc".into()),
            ..inline.clone()
        };
        assert_eq!(
            read_clip_bytes(&client, &routed, &key).await.unwrap(),
            image
        );
        let plain = Clip {
            clip_id: "plain".into(),
            encrypted: false,
            ..routed.clone()
        };
        assert_eq!(read_clip_bytes(&client, &plain, &key).await.unwrap(), image);
        let plain_inline = Clip {
            content: STANDARD.encode(image),
            encrypted: false,
            ..inline.clone()
        };
        assert_eq!(
            read_clip_bytes(&client, &plain_inline, &key).await.unwrap(),
            image
        );
        let legacy = Clip {
            content_type: "image/png".into(),
            ..plain_inline
        };
        assert!(is_binary(&legacy));
        assert_eq!(
            read_clip_bytes(&client, &legacy, &key).await.unwrap(),
            image
        );
        let legacy_encrypted = Clip {
            content_type: "image/png".into(),
            ..inline.clone()
        };
        assert_eq!(
            read_clip_bytes(&client, &legacy_encrypted, &key)
                .await
                .unwrap(),
            image
        );
        let text = Clip {
            content: "plain text".into(),
            ..Default::default()
        };
        assert_eq!(
            read_clip_bytes(&client, &text, &key).await.unwrap(),
            b"plain text"
        );
        assert!(read_clip_bytes(&client, &inline, &[9u8; 32]).await.is_err());
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
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
