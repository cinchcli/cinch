//! `cinch send` — read stdin, encrypt, and SEND to your fleet via the relay.
//!
//! The explicit fleet-send verb (broadcast to all your devices, E2EE always
//! on). Unlike `cinch copy` (local-only), `send` contacts the relay. It wires
//! the existing `LocalPusher` pipeline into the CLI: classify → key-gate →
//! encrypt → POST /clips → local write-through.
//!
//! NOW scope is broadcast-only, stdin-only: there is no `--to`/`--target` and
//! no `[REF]` positional (directed send + send-by-ref are deferred follow-ups,
//! send spec §3.1 / §11).

use std::io::Read;

use client_core::machine::hostname_or_unknown;
use client_core::rest::ContentType;
use client_core::sync::{IngestError, LocalPusher, PushOutcome};

use crate::exit::{
    ExitError, ENCRYPTION_PENDING, ENCRYPTION_REQUIRED, GENERIC_ERROR, NETWORK_ERROR,
};

const MAX_SEND_SIZE: usize = 20 * 1024 * 1024;

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
    /// automatically. The override is collapsed to the canonical wire
    /// vocabulary (text/code/url/image) before the wire — a raw MIME string
    /// never leaks.
    #[arg(long = "type")]
    pub force_type: Option<String>,

    /// Force text mode (skip binary detection).
    #[arg(long)]
    pub text: bool,

    /// Override auth token (also CINCH_TOKEN). Used to contact the relay.
    #[arg(long)]
    pub token: Option<String>,

    /// Override relay URL (also CINCH_RELAY_URL).
    #[arg(long)]
    pub relay: Option<String>,

    /// Treat an offline-queued clip as a failure (exit NETWORK_ERROR) instead
    /// of exit 0. Use in CI where a queued-but-unsent clip means the box died
    /// with the clip still local.
    #[arg(long)]
    pub require_online: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetailedType {
    Text,
    Image,
    Video,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    // --- read stdin (binary-safe) ---
    let mut data = Vec::new();
    std::io::stdin()
        .read_to_end(&mut data)
        .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Cannot read stdin: {}", e), ""))?;

    // --- guards + classify (shared with copy semantics) ---
    let plan = classify(&data, args.text, args.force_type.as_deref())?;

    // --- open relay-aware context (overlay resolver, §3.2) ---
    let ctx = crate::runtime::open_ctx_with(args.token.as_deref(), args.relay.as_deref())
        .map_err(|e| ExitError::new(crate::exit::AUTH_FAILURE, e, "Run: cinch auth login"))?;

    // --- KEY-GATE (§3.4): fail fast before LocalPusher silently queues a
    // never-encrypted clip on an ephemeral box. ---
    if ctx.enc_key.is_none() {
        return Err(no_key_error(ctx.key_pending));
    }

    let hostname = hostname_or_unknown();
    let source = format!("remote:{}", hostname);
    let label = args.label.unwrap_or_default();

    let pusher = LocalPusher::new(ctx.store.clone(), ctx.client.clone(), ctx.enc_key);
    let outcome = match plan.wire_type {
        ContentType::Image => pusher.push_image_png(data, &source, &label).await,
        ct => pusher.push_text(data, &source, &label, ct).await,
    };

    map_outcome(outcome, args.require_online, args.silent)
}

/// Stdin classification result (guards already passed).
#[derive(Debug)]
struct Plan {
    wire_type: ContentType,
}

/// Run the empty/oversize/video guards and classify into the canonical wire
/// vocabulary. Pure over the bytes so it is unit-testable without stdin.
fn classify(data: &[u8], force_text: bool, force_type: Option<&str>) -> Result<Plan, ExitError> {
    if data.is_empty() {
        return Err(ExitError::new(
            GENERIC_ERROR,
            "No input. Pipe content to cinch send.",
            "Example: echo 'hello' | cinch send",
        ));
    }
    if data.len() > MAX_SEND_SIZE {
        return Err(ExitError::new(
            GENERIC_ERROR,
            format!("Input too large: {} bytes (max 20MB).", data.len()),
            "",
        ));
    }

    let detected = detect_content_type(data);
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
    // Collapse to canonical-4 BEFORE the wire — a raw MIME (`text/plain`,
    // `image/png`) never leaks past this point.
    let wire_type = if is_binary {
        ContentType::Image
    } else if force_text {
        ContentType::Text
    } else {
        client_core::classify::detect(data)
    };

    Ok(Plan { wire_type })
}

/// The §3.4 key-gate decision: distinguish "signed in but key pending" from
/// "no key at all" using the existing exit codes.
fn no_key_error(key_pending: bool) -> ExitError {
    if key_pending {
        ExitError::new(
            ENCRYPTION_PENDING,
            "Encryption key not yet received from a paired device.",
            "Run: cinch auth retry-key",
        )
    } else {
        ExitError::new(
            ENCRYPTION_REQUIRED,
            "Encryption key missing. `cinch send` requires the AES master key on this box.",
            "Sign in (cinch auth login) or provision ~/.cinch/config.json with the key on headless boxes.",
        )
    }
}

/// Map a `LocalPusher` outcome to the CLI's exit/plane-loud contract (§3.4).
fn map_outcome(
    outcome: Result<PushOutcome, IngestError>,
    require_online: bool,
    silent: bool,
) -> Result<(), ExitError> {
    match outcome {
        Ok(PushOutcome::Synced(_id)) => {
            if !silent {
                // Plane-loud (redesign §7): explicitly the FLEET plane + E2EE.
                eprintln!("\u{2713} Sent to your fleet (E2EE)");
            }
            Ok(())
        }
        Ok(PushOutcome::Queued(id)) => {
            // A queue is not a success the user should miss — always print,
            // even with --silent.
            eprintln!("\u{26A0} Queued offline — will retry on next cinch command (id={id})");
            if require_online {
                Err(ExitError::new(
                    NETWORK_ERROR,
                    "Clip queued offline (relay unreachable); --require-online set.",
                    "Retry when the relay is reachable.",
                ))
            } else {
                Ok(())
            }
        }
        // Permanent push errors carry correct codes via From<HttpError>.
        Err(IngestError::Push(e)) => Err(ExitError::from(e)),
        // Crypto / store / not-found / unreachable-no-key → generic.
        Err(IngestError::Crypto(m)) => Err(ExitError::new(
            GENERIC_ERROR,
            format!("encryption failed: {m}"),
            "",
        )),
        Err(IngestError::Store(e)) => Err(ExitError::new(
            GENERIC_ERROR,
            format!("local store write failed: {e}"),
            "",
        )),
        Err(IngestError::NotFound(m)) => {
            Err(ExitError::new(GENERIC_ERROR, format!("not found: {m}"), ""))
        }
        // Unreachable on push_text/push_image_png (they queue instead), and the
        // §3.4 key-gate makes it moot anyway — mapped defensively.
        Err(IngestError::NoEncryptionKey) => Err(no_key_error(false)),
    }
}

/// `--type` accepts either canonical `image` or any `image/*` MIME.
fn force_is_image(s: &str) -> bool {
    s == "image" || s.starts_with("image/")
}

/// Sniffs image or video magic bytes; falls back to Text. Shares the same
/// detection contract as `copy`.
fn detect_content_type(data: &[u8]) -> DetailedType {
    if client_core::media::is_image(data) {
        return DetailedType::Image;
    }
    let is_video = (data.len() >= 12 && (&data[4..8] == b"ftyp" || &data[4..8] == b"moov"))
        || (data.starts_with(b"RIFF") && data.len() >= 12 && &data[8..12] == b"AVI ")
        || data.starts_with(b"\x1a\x45\xdf\xa3");
    if is_video {
        DetailedType::Video
    } else {
        DetailedType::Text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- guards + classify --------------------------------------------------

    #[test]
    fn classify_empty_is_generic_error() {
        let err = classify(b"", false, None).expect_err("empty must error");
        assert_eq!(err.code, GENERIC_ERROR);
    }

    #[test]
    fn classify_video_rejected() {
        let mp4 = b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00";
        let err = classify(mp4, false, None).expect_err("video must reject");
        assert_eq!(err.code, GENERIC_ERROR);
        assert!(err.message.contains("Video"));
    }

    #[test]
    fn classify_png_is_image() {
        let png = b"\x89PNG\r\n\x1a\nIHDR\x00";
        let plan = classify(png, false, None).expect("ok");
        assert_eq!(plan.wire_type, ContentType::Image);
    }

    #[test]
    fn classify_force_type_mime_does_not_leak_to_wire() {
        // `--type text/plain` must collapse to a canonical-4 value (never the
        // raw MIME). With force_text-equivalent classification, the wire type
        // is one of the canonical four.
        let plan = classify(b"hello", false, Some("text/plain")).expect("ok");
        let wire = plan.wire_type.as_wire();
        assert!(
            matches!(wire, "text" | "code" | "url" | "image"),
            "wire vocabulary must be canonical-4, got {wire:?}"
        );
        assert!(!wire.contains('/'), "no MIME slash on the wire: {wire:?}");
    }

    #[test]
    fn classify_force_type_image_mime_collapses_to_image() {
        let plan = classify(b"not really a png", false, Some("image/png")).expect("ok");
        assert_eq!(plan.wire_type, ContentType::Image);
        assert_eq!(plan.wire_type.as_wire(), "image");
    }

    #[test]
    fn classify_force_text_skips_binary_detection() {
        // PNG magic bytes, but --text forces Text classification.
        let png = b"\x89PNG\r\n\x1a\nIHDR\x00";
        let plan = classify(png, true, None).expect("ok");
        assert_eq!(plan.wire_type, ContentType::Text);
    }

    // --- key-gate -----------------------------------------------------------

    #[test]
    fn no_key_not_pending_is_encryption_required() {
        let err = no_key_error(false);
        assert_eq!(err.code, ENCRYPTION_REQUIRED);
        assert!(err.message.to_lowercase().contains("missing"));
    }

    #[test]
    fn no_key_pending_is_encryption_pending() {
        let err = no_key_error(true);
        assert_eq!(err.code, ENCRYPTION_PENDING);
        assert!(err.fix.contains("cinch auth retry-key"));
    }

    // --- outcome mapping ----------------------------------------------------

    #[test]
    fn synced_outcome_is_success() {
        let res = map_outcome(Ok(PushOutcome::Synced("clip-1".into())), false, true);
        assert!(res.is_ok());
    }

    #[test]
    fn queued_outcome_default_is_success() {
        let res = map_outcome(Ok(PushOutcome::Queued("local-1".into())), false, true);
        assert!(res.is_ok(), "queued without --require-online is exit 0");
    }

    #[test]
    fn queued_outcome_require_online_is_network_error() {
        let res = map_outcome(Ok(PushOutcome::Queued("local-1".into())), true, true);
        let err = res.expect_err("--require-online turns Queued into a failure");
        assert_eq!(err.code, NETWORK_ERROR);
    }

    #[test]
    fn permanent_push_error_maps_via_http_error() {
        // A 401 → AUTH_FAILURE through From<HttpError>.
        let res = map_outcome(
            Err(IngestError::Push(
                client_core::http::HttpError::Unauthorized,
            )),
            false,
            true,
        );
        let err = res.expect_err("permanent error surfaces");
        assert_eq!(err.code, crate::exit::AUTH_FAILURE);
    }

    #[test]
    fn crypto_error_is_generic() {
        let res = map_outcome(Err(IngestError::Crypto("bad key".into())), false, true);
        assert_eq!(res.expect_err("crypto error").code, GENERIC_ERROR);
    }

    // Note: LocalPusher's queue-on-no-key and synced-on-recording behaviors are
    // covered by client_core's own #[cfg(test)] tests (the for_test_* RestClient
    // constructors are test-gated and not reachable cross-crate). At the CLI
    // layer we test what `send` owns: the classify guards, the §3.4 key-gate
    // decision (no_key_error), and the PushOutcome -> ExitError mapping above.
}
