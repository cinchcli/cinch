//! Anonymous, opt-out usage telemetry for the cinch CLI.
//!
//! Active only when the binary was built with `CINCH_TELEMETRY_KEY` and
//! `CINCH_TELEMETRY_URL` env vars present at compile time. Source-built
//! binaries (no env vars at build time) compile this module to a permanent
//! no-op — `is_enabled()` returns false and every entry point short-circuits.
//!
//! Runtime opt-out: `TELEMETRY_DISABLED=1`, `DO_NOT_TRACK=1`, the file
//! `~/.cinch/telemetry_opt_out`, or `cinch telemetry off`.
//!
//! See `cinchcli.com/telemetry` for the exhaustive list of what is and
//! isn't collected.

mod backend_posthog;
mod event;
mod id;

use std::sync::OnceLock;
use std::time::Duration;

pub use event::Event;

use backend_posthog::PostHogBackend as Backend;

/// Build-time vendor key. `None` for any binary built without the env var.
const TELEMETRY_KEY: Option<&str> = option_env!("CINCH_TELEMETRY_KEY");

/// Build-time vendor endpoint. `None` for any binary built without the env var.
const TELEMETRY_URL: Option<&str> = option_env!("CINCH_TELEMETRY_URL");

static CLIENT: OnceLock<Option<Backend>> = OnceLock::new();

pub struct Status {
    pub compiled_in: bool,
    pub env_disabled: bool,
    pub do_not_track: bool,
    pub opt_out_file: bool,
    pub active: bool,
}

/// Initializes the telemetry subsystem. Idempotent.
///
/// On the very first run with telemetry compiled in and not opted out, prints
/// a one-line notice to stderr and creates `~/.cinch/telemetry_id`.
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
            print_first_run_notice();
        }
        Some(Backend::new(url, key, distinct_id))
    });
}

/// True if telemetry would actually send events.
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

/// Buffers an event. Non-blocking, never fails, no-op when disabled.
pub fn capture(event: Event) {
    if let Some(Some(backend)) = CLIENT.get() {
        backend.capture(event);
    }
}

/// Associates future events with `user_id` and emits a `$identify` event
/// merging the anonymous distinct_id into the user's PostHog person.
pub fn identify(user_id: &str) {
    if let Some(Some(backend)) = CLIENT.get() {
        backend.identify(user_id);
    }
}

/// Flushes the buffer with a hard timeout. Failures are silent.
pub async fn flush(timeout: Duration) {
    let Some(Some(backend)) = CLIENT.get() else {
        return;
    };
    let _ = tokio::time::timeout(timeout, backend.flush()).await;
}

/// Snapshot of the current telemetry state for `cinch telemetry status`.
pub fn status() -> Status {
    Status {
        compiled_in: TELEMETRY_KEY.is_some() && TELEMETRY_URL.is_some(),
        env_disabled: std::env::var_os("TELEMETRY_DISABLED").is_some(),
        do_not_track: std::env::var_os("DO_NOT_TRACK").is_some(),
        opt_out_file: id::opt_out_file_path().exists(),
        active: is_enabled(),
    }
}

/// Toggles `~/.cinch/telemetry_opt_out`.
pub fn set_opt_out(opt_out: bool) -> std::io::Result<()> {
    id::set_opt_out_file(opt_out)
}

fn print_first_run_notice() {
    eprintln!(
        "cinch sends anonymous usage stats to help improve the tool. \
         No PII, no clipboard contents. \
         Opt out: TELEMETRY_DISABLED=1 or `cinch telemetry off`. \
         Details: https://cinchcli.com/telemetry"
    );
}
