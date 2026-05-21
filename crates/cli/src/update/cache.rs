//! Read/write `~/.cinch/update-check.json`.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cache {
    pub last_check_unix: i64,
    pub latest_version: String,
    pub latest_published_unix: i64,
}

/// Resolves to `~/.cinch/update-check.json` on every supported platform.
pub fn default_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".cinch").join("update-check.json"))
}

pub fn read(path: &Path) -> Option<Cache> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn write(path: &Path, cache: &Cache) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }
    let bytes = serde_json::to_vec(cache)?;
    fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn round_trips() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("update-check.json");
        let original = Cache {
            last_check_unix: 1_715_000_000,
            latest_version: "0.6.0".to_string(),
            latest_published_unix: 1_714_900_000,
        };
        write(&path, &original).unwrap();
        let read_back = read(&path).unwrap();
        assert_eq!(read_back, original);
    }

    #[test]
    fn read_returns_none_on_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        assert!(read(&path).is_none());
    }

    #[test]
    fn read_returns_none_on_malformed_json() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.json");
        fs::write(&path, b"{not json").unwrap();
        assert!(read(&path).is_none());
    }
}
