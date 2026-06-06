//! Anonymous, opt-IN usage telemetry for the cinch desktop app.
//!
//! Telemetry is **off by default**. It is enabled only after the user opts in
//! via the shared `~/.cinch/telemetry_opt_in` file (the same file the CLI's
//! `cinch account telemetry on` toggles, so consent is one-per-machine). When
//! enabled, events are POSTed to the app's OWN configured relay at
//! `POST {relay_url}/telemetry/otlp`; the relay anonymizes (HMACs the id) and
//! forwards them to the observability stack. There is no vendor backend and no
//! build-time key — telemetry is gated purely at runtime by the opt-in file.
//!
//! Force-off overrides (kept for convention): `TELEMETRY_DISABLED=1` or
//! `DO_NOT_TRACK=1` disable telemetry even when opted in.
//!
//! Differences from the CLI: this is a long-lived process, so events flush from
//! a background task (every `FLUSH_INTERVAL`) and from a `FLUSH_THRESHOLD`
//! trigger in `capture`, rather than blocking on process exit.
//!
//! See `cinchcli.com/telemetry` for the exhaustive list of what is and isn't
//! collected.

mod backend_otlp;
mod event;
mod id;

use std::sync::{Arc, OnceLock};
use std::time::Duration;

pub use event::Event;

use backend_otlp::OtlpBackend as Backend;

const FLUSH_INTERVAL: Duration = Duration::from_secs(30);
const FLUSH_THRESHOLD: usize = 20;

static CLIENT: OnceLock<Option<Arc<Backend>>> = OnceLock::new();

/// Initializes telemetry and spawns the background flush task. Idempotent.
///
/// When telemetry is disabled (the default), this stores `None` and every entry
/// point short-circuits. When opted in, it loads/creates the anonymous id and
/// resolves the destination relay from the active profile. An opted-in user with
/// no active relay still initializes successfully — `flush()` simply becomes a
/// no-op because there is nowhere to send to.
pub fn init() {
    CLIENT.get_or_init(|| {
        if !is_enabled_inner() {
            return None;
        }
        let is_first_run = !id::id_file_path().exists();
        let anon_id = id::load_or_create().ok()?;
        // Resolve the destination relay from the same on-disk config the rest of
        // run() loads (lib.rs:141). No active profile → empty base → flush no-op.
        let relay_base = crate::protocol::MultiConfig::load()
            .active_profile()
            .map(|p| p.relay_url.clone())
            .unwrap_or_default();
        if is_first_run {
            log::info!(
                "Cinch desktop telemetry: anonymous usage stats ENABLED (you opted in). \
                 Disable: cinch account telemetry off or DO_NOT_TRACK=1. \
                 See https://cinchcli.com/telemetry."
            );
        }
        let backend = Arc::new(Backend::new(relay_base, anon_id));
        spawn_flush_task(backend.clone());
        Some(backend)
    });
}

pub fn is_enabled() -> bool {
    is_enabled_inner()
}

fn is_enabled_inner() -> bool {
    let force_off = std::env::var_os("TELEMETRY_DISABLED").is_some()
        || std::env::var_os("DO_NOT_TRACK").is_some();
    gate_enabled(force_off, id::opt_in_file_path().exists())
}

/// Pure opt-in gate: force-off overrides everything; otherwise enabled only when
/// the user has opted in. Default (no opt-in) is OFF.
fn gate_enabled(force_off: bool, opted_in: bool) -> bool {
    if force_off {
        return false;
    }
    opted_in
}

/// Buffers an event. Non-blocking, never fails, no-op when disabled. When the
/// buffer crosses `FLUSH_THRESHOLD`, a fire-and-forget flush is spawned so a
/// burst of events does not wait for the next interval tick.
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

/// Reserved for a future user_ref join. The OTLP backend does not associate
/// identity (the relay only ever sees the HMAC'd anon id), so this is a retained
/// no-op kept only so existing callers compile.
pub fn identify(_user_id: &str) {}

/// Best-effort final flush. Called on app shutdown. Failures are silent.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_by_default_without_opt_in() {
        // No opt-in file, no force-off → telemetry is OFF.
        assert!(!gate_enabled(false, false));
    }

    #[test]
    fn enabled_when_opted_in() {
        assert!(gate_enabled(false, true));
    }

    #[test]
    fn force_off_overrides_opt_in() {
        // DO_NOT_TRACK / TELEMETRY_DISABLED wins even when opted in.
        assert!(!gate_enabled(true, true));
    }

    #[test]
    fn force_off_with_no_opt_in_is_off() {
        assert!(!gate_enabled(true, false));
    }
}
