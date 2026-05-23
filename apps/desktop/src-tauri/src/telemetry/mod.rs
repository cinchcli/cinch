//! Anonymous, opt-out usage telemetry for the cinch desktop app.
//!
//! Mirrors the CLI module's design (`cinch/crates/cli/src/telemetry/`):
//! build-time gated via `CINCH_TELEMETRY_KEY` and `CINCH_TELEMETRY_URL`,
//! shared `~/.cinch/telemetry_id` with the CLI so one user = one person in
//! the dashboard, vendor-specific code confined to `backend_posthog.rs`.
//!
//! Differences from CLI: long-lived process, so events flush from a
//! background task (every 30s) rather than blocking on process exit.
//!
//! Runtime opt-out: `TELEMETRY_DISABLED=1`, `DO_NOT_TRACK=1`, the file
//! `~/.cinch/telemetry_opt_out`. (The desktop has no `cinch account telemetry off`
//! analogue yet — toggling is currently CLI-only or by touching the file.)

mod backend_posthog;
mod event;
mod id;

use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub use event::Event;

use backend_posthog::PostHogBackend as Backend;

const TELEMETRY_KEY: Option<&str> = option_env!("CINCH_TELEMETRY_KEY");
const TELEMETRY_URL: Option<&str> = option_env!("CINCH_TELEMETRY_URL");
const FLUSH_INTERVAL: Duration = Duration::from_secs(30);
const FLUSH_THRESHOLD: usize = 20;

static CLIENT: OnceLock<Option<Arc<Backend>>> = OnceLock::new();

/// Initializes telemetry and spawns the background flush task. Idempotent.
pub fn init() {
    CLIENT.get_or_init(|| {
        if !is_enabled_inner() {
            return None;
        }
        let key = TELEMETRY_KEY?;
        let url = TELEMETRY_URL?;
        let is_first_run = !id::id_file_path().exists();
        let distinct_id = id::load_or_create().ok()?;
        if is_first_run {
            log::info!(
                "Cinch desktop telemetry: anonymous usage stats enabled. \
                 Opt out by touching ~/.cinch/telemetry_opt_out or setting \
                 TELEMETRY_DISABLED=1. See https://cinchcli.com/telemetry."
            );
        }
        let backend = Arc::new(Backend::new(url, key, distinct_id));
        spawn_flush_task(backend.clone());
        Some(backend)
    });
}

pub fn is_enabled() -> bool {
    is_enabled_inner()
}

fn is_enabled_inner() -> bool {
    if TELEMETRY_KEY.is_none() || TELEMETRY_URL.is_none() {
        return false;
    }
    if std::env::var_os("TELEMETRY_DISABLED").is_some() {
        return false;
    }
    if std::env::var_os("DO_NOT_TRACK").is_some() {
        return false;
    }
    if id::opt_out_file_path().exists() {
        return false;
    }
    true
}

pub fn capture(event: Event) {
    let Some(Some(backend)) = CLIENT.get() else {
        return;
    };
    backend.capture(event);
    if backend.buffer_len() >= FLUSH_THRESHOLD {
        let b = backend.clone();
        tauri::async_runtime::spawn(async move {
            b.flush().await;
        });
    }
}

pub fn identify(user_id: &str) {
    if let Some(Some(backend)) = CLIENT.get() {
        backend.identify(user_id);
    }
}

/// Best-effort final flush. Called on app shutdown.
pub async fn shutdown_flush(timeout: Duration) {
    let Some(Some(backend)) = CLIENT.get() else {
        return;
    };
    let _ = tokio::time::timeout(timeout, backend.flush()).await;
}

fn spawn_flush_task(backend: Arc<Backend>) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(FLUSH_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Burn the immediate first tick — nothing to flush at startup.
        tick.tick().await;
        loop {
            tick.tick().await;
            backend.flush().await;
        }
    });
}
