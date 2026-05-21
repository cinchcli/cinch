//! Detect Cinch.app on the local machine and hand the OAuth flow off to it.
//!
//! When the desktop app is installed, the CLI's `auth login` opens a
//! `cinch://login` deep link instead of running the device-code flow itself.
//! The desktop catches the deep link, focuses its sign-in dialog, and
//! writes credentials to `~/.cinch/config.json`. The CLI watches the file
//! for a `credential_version` bump and prints "Signed in via Cinch.app."
//!
//! Falls back to the CLI's own device-code flow when the desktop is not
//! installed or does not respond within the handoff window.

use std::time::{Duration, Instant};

use client_core::auth::load_multi_config;

const HANDOFF_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Result of attempting a handoff to the desktop app.
#[derive(Debug)]
pub enum HandoffOutcome {
    /// Desktop wrote a fresh credential set and the CLI adopted it.
    Adopted { user_id: String, relay_url: String },
    /// The handoff window elapsed without a `credential_version` bump. The
    /// caller should fall back to the CLI's own device-code flow.
    TimedOut,
}

/// Returns true when Cinch.app appears to be the registered handler for the
/// `cinch://` URL scheme on this machine. On macOS the canonical signal is
/// Launch Services; we approximate it by checking for `/Applications/Cinch.app`
/// (and the user-local Applications folder), which is sufficient for the
/// "is the desktop installed?" question. On non-Mac platforms returns false
/// for now — desktop only ships for macOS.
pub fn desktop_is_default_handler_for_cinch_scheme() -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::path::Path;
        let candidates = ["/Applications/Cinch.app", "/Applications/cinch.app"];
        for c in candidates {
            if Path::new(c).exists() {
                return true;
            }
        }
        if let Some(home) = dirs::home_dir() {
            let user_app = home.join("Applications").join("Cinch.app");
            if user_app.exists() {
                return true;
            }
        }
        false
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Open `cinch://login?relay=…&from=cli` and wait up to 120 seconds for the
/// desktop to write credentials to `~/.cinch/config.json`. Adoption is
/// signalled by `credential_version` strictly increasing past `baseline_version`.
pub async fn handoff_login(relay_url: &str) -> HandoffOutcome {
    let baseline_version = current_version();

    let url = format!("cinch://login?relay={}&from=cli", urlencode(relay_url));
    let _ = open::that(&url);

    let deadline = Instant::now() + HANDOFF_TIMEOUT;
    while Instant::now() < deadline {
        tokio::time::sleep(POLL_INTERVAL).await;
        if let Some(profile) = load_multi_config()
            .ok()
            .and_then(|mc| mc.active_profile().cloned())
        {
            if profile.credential_version > baseline_version
                && !profile.user_id.is_empty()
                && !profile.device_id.is_empty()
                && !profile.token.is_empty()
            {
                return HandoffOutcome::Adopted {
                    user_id: profile.user_id,
                    relay_url: profile.relay_url,
                };
            }
        }
    }
    HandoffOutcome::TimedOut
}

fn current_version() -> u64 {
    load_multi_config()
        .ok()
        .and_then(|mc| mc.active_profile().map(|p| p.credential_version))
        .unwrap_or(0)
}

/// Minimal URL component encoder for the values we put into
/// `cinch://login?relay=...&from=cli`. Avoids pulling `percent-encoding` in
/// just for one call site.
fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        let safe = matches!(
            b,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'
        );
        if safe {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencode_handles_special_chars() {
        assert_eq!(
            urlencode("https://api.cinchcli.com"),
            "https%3A%2F%2Fapi.cinchcli.com"
        );
        assert_eq!(urlencode("plain"), "plain");
    }
}
