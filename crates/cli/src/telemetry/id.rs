//! Per-user anonymous id management plus the opt-in / hint-marker files.
//!
//! - `~/.cinch/telemetry_id` — a single line containing a UUID v7. Anonymous,
//!   client-generated, HMAC'd relay-side; not identity.
//! - `~/.cinch/telemetry_opt_in` — presence enables telemetry (default OFF).
//! - `~/.cinch/telemetry_hint_shown` — marker so the one-time discovery hint
//!   never prints twice.

use std::fs;
use std::io;
use std::path::PathBuf;

use uuid::Uuid;

const ID_FILE: &str = "telemetry_id";
const OPT_IN_FILE: &str = "telemetry_opt_in";
const HINT_SHOWN_FILE: &str = "telemetry_hint_shown";

fn cinch_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".cinch"))
}

pub fn opt_in_file_path() -> PathBuf {
    cinch_dir().unwrap_or_default().join(OPT_IN_FILE)
}

pub fn hint_shown_path() -> PathBuf {
    cinch_dir().unwrap_or_default().join(HINT_SHOWN_FILE)
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

/// Creates or removes `~/.cinch/telemetry_opt_in`.
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

/// True once the one-time discovery hint has been shown.
pub fn hint_shown() -> bool {
    hint_shown_path().exists()
}

/// Records that the one-time discovery hint has been shown. Best-effort: a
/// failure here just risks the hint printing once more, never a hard error.
pub fn mark_hint_shown() {
    let path = hint_shown_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, b"");
}
