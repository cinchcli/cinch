//! Stable per-machine identifier used to deduplicate device rows on the relay.
//!
//! The relay uses `machine_id` to recognize when the CLI and desktop on the
//! same Mac are signing in independently and should share a single device row
//! instead of producing two. The value is opaque (SHA-256 of the OS-provided
//! UUID, truncated to 16 hex chars) so the underlying hardware identifier is
//! never sent in cleartext.
//!
//! Sources by platform:
//!   macOS   — `IOPlatformUUID` via `ioreg -rd1 -c IOPlatformExpertDevice`
//!   Linux   — `/etc/machine-id` (or `/var/lib/dbus/machine-id` fallback)
//!   Windows — `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`
//!
//! Returns an empty string when the source is unavailable; callers treat that
//! the same as "no dedup hint" and the relay falls back to today's behavior.

use sha2::{Digest, Sha256};

/// Returns true if the current process appears to be running inside an SSH
/// session, based on the SSH_* environment variables OpenSSH's sshd sets
/// when forking a login shell. Suppresses browser auto-open and desktop
/// handoff during `cinch auth login`.
pub fn in_ssh_session() -> bool {
    std::env::var("SSH_CONNECTION").is_ok()
        || std::env::var("SSH_TTY").is_ok()
        || std::env::var("SSH_CLIENT").is_ok()
}

/// Returns the OS-level hostname via `gethostname(3)`, or `"unknown"` if the
/// syscall fails or yields an empty value. Identical on CLI and desktop so
/// the relay's source_key dedup matches across both clients on the same Mac.
pub fn hostname_or_unknown() -> String {
    hostname::get()
        .ok()
        .and_then(|os| os.into_string().ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Returns the `source` value used when this device pushes a clip.
///
/// Format: `remote:<hostname>`. Mirrors the format produced by
/// `cinch push` (see commands/push.rs). Used by `cinch pull --exclude-self`
/// and the underlying `--exclude-source` query so the relay can suppress
/// clips authored by the local device.
pub fn self_source_key() -> String {
    format!("remote:{}", hostname_or_unknown())
}

/// Returns a stable, opaque identifier for this machine, or an empty string
/// if no source is available. The value is suitable for sending to the relay
/// (no raw hardware ID) and is consistent across CLI and desktop on the same
/// machine.
pub fn stable_machine_id() -> String {
    match raw_machine_source() {
        Some(raw) if !raw.is_empty() => hash_short(&raw),
        _ => String::new(),
    }
}

fn hash_short(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    let mut s = String::with_capacity(16);
    for b in &digest[..8] {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

#[cfg(target_os = "macos")]
fn raw_machine_source() -> Option<String> {
    use std::process::Command;
    let out = Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("\"IOPlatformUUID\"") {
            // Format: "IOPlatformUUID" = "ABCD-..."
            if let Some(start) = rest.find('=') {
                let after = rest[start + 1..].trim();
                let unq = after.trim_matches('"');
                if !unq.is_empty() {
                    return Some(unq.to_string());
                }
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn raw_machine_source() -> Option<String> {
    for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(s) = std::fs::read_to_string(path) {
            let v = s.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn raw_machine_source() -> Option<String> {
    use std::process::Command;
    let out = Command::new("reg")
        .args([
            "query",
            "HKLM\\SOFTWARE\\Microsoft\\Cryptography",
            "/v",
            "MachineGuid",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("MachineGuid") {
            // Format: MachineGuid    REG_SZ    abcd-...
            let mut parts = trimmed.split_whitespace();
            let _ = parts.next(); // MachineGuid
            let _ = parts.next(); // REG_SZ
            if let Some(v) = parts.next() {
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn raw_machine_source() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn hash_is_16_hex_chars() {
        let h = hash_short("any-input");
        assert_eq!(h.len(), 16);
        assert!(h.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash_is_deterministic() {
        assert_eq!(hash_short("same"), hash_short("same"));
        assert_ne!(hash_short("a"), hash_short("b"));
    }

    fn with_env<F: FnOnce()>(vars: &[(&str, Option<&str>)], f: F) {
        let prev: Vec<_> = vars
            .iter()
            .map(|(k, _)| (k.to_string(), env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            match v {
                Some(val) => env::set_var(k, val),
                None => env::remove_var(k),
            }
        }
        f();
        for (k, v) in prev {
            match v {
                Some(val) => env::set_var(&k, val),
                None => env::remove_var(&k),
            }
        }
    }

    #[test]
    fn ssh_connection_triggers() {
        with_env(
            &[
                ("SSH_CONNECTION", Some("1.2.3.4 22 5.6.7.8 22")),
                ("SSH_TTY", None),
                ("SSH_CLIENT", None),
            ],
            || assert!(in_ssh_session()),
        );
    }

    #[test]
    fn ssh_tty_triggers() {
        with_env(
            &[
                ("SSH_CONNECTION", None),
                ("SSH_TTY", Some("/dev/pts/0")),
                ("SSH_CLIENT", None),
            ],
            || assert!(in_ssh_session()),
        );
    }

    #[test]
    fn ssh_client_triggers() {
        with_env(
            &[
                ("SSH_CONNECTION", None),
                ("SSH_TTY", None),
                ("SSH_CLIENT", Some("1.2.3.4 22 22")),
            ],
            || assert!(in_ssh_session()),
        );
    }

    #[test]
    fn no_ssh_env_returns_false() {
        with_env(
            &[
                ("SSH_CONNECTION", None),
                ("SSH_TTY", None),
                ("SSH_CLIENT", None),
            ],
            || assert!(!in_ssh_session()),
        );
    }

    #[test]
    fn self_source_key_matches_push_format() {
        let key = self_source_key();
        let host = hostname_or_unknown();
        assert_eq!(key, format!("remote:{}", host));
    }

    #[test]
    fn self_source_key_has_remote_prefix() {
        assert!(self_source_key().starts_with("remote:"));
    }
}
