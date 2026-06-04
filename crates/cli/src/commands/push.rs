//! `cinch push` — read stdin, save to local clip history.
//!
//! Ingests content from stdin and stores it in the local database. This
//! command is local-only; the relay is never contacted.

use std::time::Instant;

use client_core::auth::load_config;
use client_core::config::Config;
use client_core::machine::hostname_or_unknown;
use client_core::store::models::{StoredClip, SyncState};
use client_core::store::{self, queries, Store};

use crate::commands::shared::{format_bytes, read_and_classify_stdin};
use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

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

pub async fn run(args: Args) -> Result<(), ExitError> {
    // Note: no `ensure_authenticated()` guard here. `resolve_config` below
    // overlays `--token` and `CINCH_TOKEN` on top of disk state and then
    // emits the same `AUTH_FAILURE` + `Run: cinch auth login` error when
    // every source is empty, so adding the guard would override the
    // documented stateless-push path (CI / containers without `~/.cinch`).
    let _cfg = resolve_config(&args)?;

    let (data, wire_type) = read_and_classify_stdin("push", args.text, args.force_type.as_deref())?;

    let hostname = hostname_or_unknown();
    let source = format!("remote:{}", hostname);

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
