//! Anonymous, opt-IN usage telemetry for the cinch CLI.
//!
//! Telemetry is **off by default**. It is enabled only after the user runs
//! `cinch account telemetry on` (which creates `~/.cinch/telemetry_opt_in`).
//! When enabled, events are POSTed to the user's OWN configured relay at
//! `POST {relay_url}/telemetry/otlp`; the relay anonymizes (HMACs the id) and
//! forwards them to the observability stack. There is no vendor backend and no
//! build-time key — telemetry is gated purely at runtime by the opt-in file.
//!
//! Force-off overrides (kept for convention): `TELEMETRY_DISABLED=1` or
//! `DO_NOT_TRACK=1` disable telemetry even when opted in.
//!
//! See `cinchcli.com/telemetry` for the exhaustive list of what is and isn't
//! collected.

mod backend_otlp;
mod event;
mod id;

use std::sync::OnceLock;
use std::time::Duration;

pub use event::Event;

use backend_otlp::OtlpBackend as Backend;

static CLIENT: OnceLock<Option<Backend>> = OnceLock::new();

pub struct Status {
    /// True when the opt-in file is present.
    pub opted_in: bool,
    /// The active relay host events would be sent to, if any.
    pub destination: Option<String>,
    pub env_disabled: bool,
    pub do_not_track: bool,
    pub active: bool,
}

/// Initializes the telemetry subsystem. Idempotent.
///
/// When telemetry is disabled (the default), this stores `None` and every entry
/// point short-circuits. When NOT opted in, it also prints a one-time discovery
/// hint to stderr (gated by `~/.cinch/telemetry_hint_shown`).
///
/// When opted in, it loads/creates the anonymous id and resolves the
/// destination relay from the active profile. An opted-in user with no active
/// relay still initializes successfully — `flush()` simply becomes a no-op
/// because there is nowhere to send to.
pub fn init() {
    CLIENT.get_or_init(|| {
        if !is_enabled_inner() {
            maybe_print_discovery_hint();
            return None;
        }
        let id = id::load_or_create().ok()?;
        let relay_base = active_relay_url().unwrap_or_default();
        Some(Backend::new(relay_base, id))
    });
}

/// True if telemetry would actually send events.
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

/// Resolves the active relay's base URL from on-disk config, if any.
fn active_relay_url() -> Option<String> {
    client_core::config::MultiConfig::load()
        .active_profile()
        .map(|p| p.relay_url.clone())
}

/// Returns the host component of the active relay URL, for display in status and
/// the `telemetry on` confirmation. Falls back to the raw URL if it has no
/// recognizable host. Kept dependency-free (no `url` crate) — this is display
/// only, so a best-effort `scheme://host[:port]/...` split is sufficient.
fn active_relay_host() -> Option<String> {
    active_relay_url().map(|url| host_of(&url))
}

/// Extracts the `host[:port]` from a URL string for display. Strips the scheme,
/// any userinfo, and the path/query; returns the input unchanged if it doesn't
/// look like a URL.
fn host_of(url: &str) -> String {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or(after_scheme);
    let host = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    if host.is_empty() {
        url.to_string()
    } else {
        host.to_string()
    }
}

/// Buffers an event. Non-blocking, never fails, no-op when disabled.
pub fn capture(event: Event) {
    if let Some(Some(backend)) = CLIENT.get() {
        backend.capture(event);
    }
}

/// Reserved for a future user_ref join. The OTLP backend does not associate
/// identity, so this is a retained no-op kept only so existing callers compile.
pub fn identify(_user_id: &str) {}

/// Flushes the buffer with a hard timeout. Failures are silent.
pub async fn flush(timeout: Duration) {
    let Some(Some(backend)) = CLIENT.get() else {
        return;
    };
    let _ = tokio::time::timeout(timeout, backend.flush()).await;
}

/// Snapshot of the current telemetry state for `cinch account telemetry status`.
pub fn status() -> Status {
    Status {
        opted_in: id::opt_in_file_path().exists(),
        destination: active_relay_host(),
        env_disabled: std::env::var_os("TELEMETRY_DISABLED").is_some(),
        do_not_track: std::env::var_os("DO_NOT_TRACK").is_some(),
        active: is_enabled(),
    }
}

/// Toggles `~/.cinch/telemetry_opt_in`.
pub fn set_opt_in(opt_in: bool) -> std::io::Result<()> {
    id::set_opt_in_file(opt_in)
}

/// Prints a one-time discovery hint to stderr when the user is not opted in.
///
/// Suppressed entirely when DO_NOT_TRACK is set (the user has signaled intent),
/// when the hint marker already exists, or when already opted in. The marker is
/// created right after printing so the hint never appears again.
fn maybe_print_discovery_hint() {
    if std::env::var_os("DO_NOT_TRACK").is_some() {
        return;
    }
    if id::opt_in_file_path().exists() {
        return;
    }
    if id::hint_shown() {
        return;
    }
    eprintln!(
        "cinch can send anonymous usage stats to help improve it (off by default). \
         Enable: cinch account telemetry on  \u{b7}  https://cinchcli.com/telemetry"
    );
    id::mark_hint_shown();
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

    #[test]
    fn host_of_extracts_host_and_port() {
        assert_eq!(host_of("https://api.cinchcli.com/v1"), "api.cinchcli.com");
        assert_eq!(host_of("http://localhost:8080"), "localhost:8080");
        assert_eq!(
            host_of("http://localhost:8080/telemetry/otlp"),
            "localhost:8080"
        );
        assert_eq!(
            host_of("https://user@relay.example.com:9000/x"),
            "relay.example.com:9000"
        );
        // No scheme / not a URL → returned as-is.
        assert_eq!(host_of("relay.example.com"), "relay.example.com");
    }
}
