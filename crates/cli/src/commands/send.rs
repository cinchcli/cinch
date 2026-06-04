//! `cinch send` — read stdin, encrypt + push to the relay (broadcast to all
//! the user's devices), record the clip locally, and copy it to this machine's
//! system clipboard.
//!
//! The headless cross-machine **write** counterpart to `cinch pull`. Unlike
//! `cinch push` (local-only; the relay is never contacted), `send` contacts the
//! relay. It is a thin wrapper over `client_core::sync::LocalPusher` — the same
//! pipeline the desktop "Send" action uses — so the encrypt / push /
//! local-write-through logic lives in exactly one place.

use std::sync::Arc;
use std::time::Instant;

use client_core::auth::load_config;
use client_core::credstore;
use client_core::http::RestClient;
use client_core::machine::hostname_or_unknown;
use client_core::rest::ContentType;
use client_core::store::{self, Store};
use client_core::sync::local_pusher::{LocalPusher, PushOutcome};
use client_core::transport::ClipTransport;

use crate::commands::shared::{format_bytes, read_and_classify_stdin};
use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};
use crate::io::copy_text_to_clipboard;

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
    /// automatically.
    #[arg(long = "type")]
    pub force_type: Option<String>,

    /// Force text mode (skip binary detection).
    #[arg(long)]
    pub text: bool,

    /// Skip copying the sent content to this machine's system clipboard.
    /// The relay broadcast and the local-history record still happen.
    #[arg(long = "no-copy")]
    pub no_copy: bool,

    /// Override auth token (also reads `CINCH_TOKEN`).
    #[arg(long)]
    pub token: Option<String>,

    /// Override relay URL (also reads `CINCH_RELAY_URL`).
    #[arg(long)]
    pub relay: Option<String>,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    // `send` is a true cross-machine write: a token + relay are required (no
    // stateless local path like `push` has).
    crate::auth_state::ensure_authenticated()?;

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

    let (data, wire_type) = read_and_classify_stdin("send", args.text, args.force_type.as_deref())?;
    let original_size = data.len() as i64;
    let is_image = matches!(wire_type, ContentType::Image);

    // Capture the text to put on the clipboard before `data` is moved into the
    // pusher. Decision: relay + local store + clipboard; clipboard is text-only
    // in this cut (no image-clipboard helper in the CLI yet).
    let clipboard = clipboard_text(args.no_copy, wire_type, &data);

    let source = format!("remote:{}", hostname_or_unknown());
    let label = args.label.clone().unwrap_or_default();

    // Build the relay client + local store and hand them to the shared pusher.
    let client = RestClient::new(
        cfg.relay_url.clone(),
        cfg.token.clone(),
        crate::client_info::for_cli(),
    )
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not init client: {}", e), ""))?;
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
    let enc_key = credstore::read_encryption_key(&cfg.user_id);
    let pusher = LocalPusher::new(
        Arc::new(store),
        Arc::new(client) as Arc<dyn ClipTransport>,
        enc_key,
    );

    let start = Instant::now();
    let outcome = if is_image {
        pusher.push_image_png(data, &source, &label).await
    } else {
        pusher.push_text(data, &source, &label, wire_type).await
    }
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Send failed: {}", e), ""))?;

    // Nudge a same-machine desktop to refresh its history immediately, the same
    // way `cinch push` does, instead of waiting for the relay's WS round-trip.
    let signal_path = store_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new(""))
        .join("local_push.signal");
    let _ = std::fs::write(&signal_path, b"1");

    let copied = match &clipboard {
        Some(text) => copy_text_to_clipboard(text),
        None => false,
    };

    if !args.silent {
        eprintln!(
            "{}",
            success_message(
                &outcome,
                original_size,
                copied,
                is_image,
                start.elapsed().as_millis()
            )
        );
    }
    Ok(())
}

/// Text to place on the system clipboard, or `None` to skip the copy. Skips
/// when `--no-copy` is set or the payload is an image (no CLI image-clipboard
/// helper in this cut). Pure so the copy decision is unit-testable without
/// touching the real clipboard.
fn clipboard_text(no_copy: bool, wire_type: ContentType, data: &[u8]) -> Option<String> {
    if no_copy || matches!(wire_type, ContentType::Image) {
        return None;
    }
    Some(String::from_utf8_lossy(data).into_owned())
}

/// Build the human-facing success line. Pure so message selection is
/// unit-testable. `Synced` confirms relay delivery; `Queued` is the
/// soft-success offline path (clip saved locally, retried later).
fn success_message(
    outcome: &PushOutcome,
    size: i64,
    copied: bool,
    is_image: bool,
    ms: u128,
) -> String {
    match outcome {
        PushOutcome::Synced(id) => {
            let mut s = format!("\u{2713} Sent {} to your devices", format_bytes(size));
            if copied {
                s.push_str(" \u{00B7} copied");
            } else if is_image {
                s.push_str(" \u{00B7} (image; clipboard not set)");
            }
            s.push_str(&format!(" \u{00B7} {} ms (id={})", ms, id));
            s
        }
        PushOutcome::Queued(_) => {
            let prefix = if copied { "Copied + saved" } else { "Saved" };
            format!(
                "\u{2713} {} locally \u{00B7} queued for your devices (offline) \u{2014} syncs when a cinch app reconnects.",
                prefix
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- clipboard_text -----------------------------------------------------

    #[test]
    fn clipboard_text_returns_text_payload() {
        let got = clipboard_text(false, ContentType::Text, b"hello");
        assert_eq!(got.as_deref(), Some("hello"));
    }

    #[test]
    fn clipboard_text_classifies_url_and_code_as_copyable_text() {
        assert_eq!(
            clipboard_text(false, ContentType::Url, b"https://x.dev").as_deref(),
            Some("https://x.dev")
        );
        assert_eq!(
            clipboard_text(false, ContentType::Code, b"fn main() {}").as_deref(),
            Some("fn main() {}")
        );
    }

    #[test]
    fn clipboard_text_skips_when_no_copy() {
        assert_eq!(clipboard_text(true, ContentType::Text, b"hello"), None);
    }

    #[test]
    fn clipboard_text_skips_for_image() {
        // Image payloads are never put on the clipboard in this cut.
        assert_eq!(clipboard_text(false, ContentType::Image, b"\x89PNG"), None);
        // `--no-copy` is moot for images, but must still be None.
        assert_eq!(clipboard_text(true, ContentType::Image, b"\x89PNG"), None);
    }

    // --- success_message ----------------------------------------------------

    #[test]
    fn success_message_synced_text_copied() {
        let msg = success_message(&PushOutcome::Synced("01ABC".into()), 5, true, false, 12);
        assert!(msg.contains("Sent"), "{msg}");
        assert!(msg.contains("to your devices"), "{msg}");
        assert!(msg.contains("copied"), "{msg}");
        assert!(msg.contains("id=01ABC"), "{msg}");
    }

    #[test]
    fn success_message_synced_text_no_copy_omits_copied() {
        let msg = success_message(&PushOutcome::Synced("01ABC".into()), 5, false, false, 12);
        assert!(msg.contains("Sent"), "{msg}");
        assert!(!msg.contains("copied"), "{msg}");
        assert!(!msg.contains("image"), "{msg}");
    }

    #[test]
    fn success_message_synced_image_notes_clipboard_not_set() {
        let msg = success_message(&PushOutcome::Synced("01ABC".into()), 2048, false, true, 30);
        assert!(msg.contains("Sent"), "{msg}");
        assert!(msg.contains("image"), "{msg}");
        assert!(msg.contains("clipboard not set"), "{msg}");
    }

    #[test]
    fn success_message_queued_copied_is_soft_success() {
        let msg = success_message(&PushOutcome::Queued("local-01".into()), 5, true, false, 4);
        assert!(msg.contains("Copied + saved"), "{msg}");
        assert!(msg.contains("queued"), "{msg}");
        assert!(msg.contains("offline"), "{msg}");
    }

    #[test]
    fn success_message_queued_no_copy_drops_copied_prefix() {
        let msg = success_message(&PushOutcome::Queued("local-01".into()), 5, false, false, 4);
        assert!(msg.contains("Saved locally"), "{msg}");
        assert!(!msg.contains("Copied"), "{msg}");
        assert!(msg.contains("queued"), "{msg}");
    }
}
