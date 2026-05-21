//! Per-user anonymous distinct_id management.
//!
//! Stored at `~/.cinch/telemetry_id` as a single line containing a UUID v7.
//! Opt-out flag at `~/.cinch/telemetry_opt_out` (presence disables telemetry).

use std::fs;
use std::io;
use std::path::PathBuf;

use uuid::Uuid;

const ID_FILE: &str = "telemetry_id";
const OPT_OUT_FILE: &str = "telemetry_opt_out";

fn cinch_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".cinch"))
}

pub fn id_file_path() -> PathBuf {
    cinch_dir().unwrap_or_default().join(ID_FILE)
}

pub fn opt_out_file_path() -> PathBuf {
    cinch_dir().unwrap_or_default().join(OPT_OUT_FILE)
}

/// Reads the distinct_id file, creating a new UUID v7 if absent.
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

/// Creates or removes `~/.cinch/telemetry_opt_out`.
pub fn set_opt_out_file(opt_out: bool) -> io::Result<()> {
    let path = opt_out_file_path();
    if opt_out {
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
