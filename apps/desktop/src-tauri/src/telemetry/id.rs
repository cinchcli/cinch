//! Per-user anonymous id management plus the shared opt-in file.
//!
//! Shared with the cinch CLI: both surfaces read/write the SAME files under
//! `~/.cinch`, so consent and identity are one-per-machine:
//!
//! - `~/.cinch/telemetry_id` — a single line containing a UUID v7. Anonymous,
//!   client-generated, HMAC'd relay-side; not identity.
//! - `~/.cinch/telemetry_opt_in` — presence enables telemetry (default OFF).
//!   The same file the CLI's `cinch account telemetry on/off` toggles.

use std::fs;
use std::io;
use std::path::PathBuf;

use uuid::Uuid;

const ID_FILE: &str = "telemetry_id";
const OPT_IN_FILE: &str = "telemetry_opt_in";

fn cinch_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".cinch"))
}

pub fn id_file_path() -> PathBuf {
    cinch_dir().unwrap_or_default().join(ID_FILE)
}

/// Path to the shared opt-in marker. Presence enables telemetry (default OFF).
pub fn opt_in_file_path() -> PathBuf {
    cinch_dir().unwrap_or_default().join(OPT_IN_FILE)
}

/// Reads the anonymous id file, creating a new UUID v7 if absent.
pub fn load_or_create() -> io::Result<String> {
    let dir = cinch_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory unavailable"))?;
    fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700));
    }
    let path = dir.join(ID_FILE);
    if let Ok(contents) = fs::read_to_string(&path) {
        let trimmed = contents.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }
    let new_id = Uuid::now_v7().to_string();
    fs::write(&path, &new_id)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(new_id)
}

/// Creates or removes `~/.cinch/telemetry_opt_in`, mirroring the CLI so both
/// surfaces honor the same shared consent file (one consent per machine).
///
/// Provided for parity with the CLI's `id.rs`. The desktop has no in-app
/// telemetry toggle command yet, so it is currently only exercised by tests and
/// the CLI; kept here so a future desktop settings toggle writes the SAME file.
#[allow(dead_code)]
pub fn set_opt_in_file(opt_in: bool) -> io::Result<()> {
    let path = opt_in_file_path();
    if opt_in {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, b"")?;
        Ok(())
    } else if path.exists() {
        fs::remove_file(&path)
    } else {
        Ok(())
    }
}
