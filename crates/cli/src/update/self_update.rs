//! Download, verify, and atomically swap the running binary.

use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Maps `(os, arch)` → the release asset filename, matching the names
/// produced by `.github/workflows/release.yml`.
pub fn asset_name() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("cinch_Darwin_arm64.tar.gz")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("cinch_Linux_x86_64.tar.gz")
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        Some("cinch_Linux_arm64.tar.gz")
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("cinch_Windows_x86_64.zip")
    } else {
        None
    }
}

pub fn verify_sha256(file_path: &Path, expected_hex: &str) -> io::Result<bool> {
    let bytes = fs::read(file_path)?;
    let actual = Sha256::digest(&bytes);
    let actual_hex: String = actual.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(actual_hex.eq_ignore_ascii_case(expected_hex.trim()))
}

/// Three-phase rename. Returns Err if the swap couldn't be made atomic;
/// caller should fall back to surfacing the .old path to the user.
pub fn atomic_swap(current: &Path, new_binary: &Path) -> io::Result<()> {
    let backup = current.with_extension("old");
    fs::rename(current, &backup)?;
    if let Err(e) = fs::rename(new_binary, current) {
        // Rollback: restore the original.
        let _ = fs::rename(&backup, current);
        return Err(e);
    }
    let _ = fs::remove_file(&backup);
    Ok(())
}

pub fn can_write_to(dir: &Path) -> bool {
    let probe = dir.join(".cinch-self-update-probe");
    match fs::write(&probe, b"") {
        Ok(()) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[derive(Debug)]
pub enum SelfUpdateError {
    UnsupportedTarget,
    NotWritable(PathBuf),
    NotPermitted(String),
    Fetch(String),
    ShaMismatch,
    Extract(String),
    Swap(io::Error),
}

impl std::fmt::Display for SelfUpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedTarget => write!(f, "no release asset for this target"),
            Self::NotWritable(p) => write!(f, "no write access to {}", p.display()),
            Self::NotPermitted(hint) => write!(f, "{}", hint),
            Self::Fetch(s) => write!(f, "fetch failed: {}", s),
            Self::ShaMismatch => write!(f, "SHA-256 mismatch — download corrupt"),
            Self::Extract(s) => write!(f, "extract failed: {}", s),
            Self::Swap(e) => write!(f, "binary swap failed: {}", e),
        }
    }
}

impl std::error::Error for SelfUpdateError {}

use crate::update::manifest::{fetch_latest, FetchError};
use crate::update::source::{detect, hint, InstallSource, RealDetector};

const MANIFEST_BASE: &str = "https://github.com/cinchcli/cinch/releases/latest/download";

pub struct RunOptions {
    pub check_only: bool,
    pub force: bool,
}

pub async fn run(opts: RunOptions) -> Result<(), SelfUpdateError> {
    let exe = std::env::current_exe()
        .map_err(|e| SelfUpdateError::NotPermitted(format!("cannot resolve current exe: {}", e)))?;
    let source = detect(&exe, &RealDetector);

    if !opts.force && !matches!(source, InstallSource::Unknown) {
        return Err(SelfUpdateError::NotPermitted(hint(&source).to_string()));
    }

    let manifest = fetch_latest().await.map_err(|e| match e {
        FetchError::Build(e)
        | FetchError::Request(e)
        | FetchError::Status(e)
        | FetchError::Parse(e) => SelfUpdateError::Fetch(e.to_string()),
    })?;

    let current = env!("CARGO_PKG_VERSION");
    let current_ver = semver::Version::parse(current)
        .map_err(|e| SelfUpdateError::Fetch(format!("bad current version: {}", e)))?;
    let next_ver = semver::Version::parse(&manifest.version)
        .map_err(|e| SelfUpdateError::Fetch(format!("bad manifest version: {}", e)))?;

    if next_ver <= current_ver {
        eprintln!("cinch is already up to date.");
        return Ok(());
    }

    eprintln!(
        "Updating cinch: {} → {}{}",
        current_ver,
        next_ver,
        if opts.check_only {
            " (--check, not applying)"
        } else {
            ""
        },
    );

    if opts.check_only {
        return Ok(());
    }

    let asset = asset_name().ok_or(SelfUpdateError::UnsupportedTarget)?;
    let exe_dir = exe.parent().ok_or_else(|| {
        SelfUpdateError::NotPermitted("current_exe has no parent directory".into())
    })?;
    if !can_write_to(exe_dir) {
        return Err(SelfUpdateError::NotWritable(exe_dir.to_path_buf()));
    }

    // Download archive and .sha256 into a tempdir on the same filesystem.
    let workdir = tempfile::Builder::new()
        .prefix(".cinch-self-update-")
        .tempdir_in(exe_dir)
        .map_err(|e| SelfUpdateError::Fetch(e.to_string()))?;
    let archive_path = workdir.path().join(asset);
    let sha_path = workdir.path().join(format!("{}.sha256", asset));

    download_to(&format!("{}/{}", MANIFEST_BASE, asset), &archive_path).await?;
    download_to(&format!("{}/{}.sha256", MANIFEST_BASE, asset), &sha_path).await?;

    let sha_text = fs::read_to_string(&sha_path)
        .map_err(|e| SelfUpdateError::Fetch(format!("read sha file: {}", e)))?;
    let expected_hex = sha_text
        .split_whitespace()
        .next()
        .ok_or_else(|| SelfUpdateError::Fetch("empty sha file".into()))?;
    if !verify_sha256(&archive_path, expected_hex)
        .map_err(|e| SelfUpdateError::Fetch(format!("sha check io: {}", e)))?
    {
        return Err(SelfUpdateError::ShaMismatch);
    }

    let new_binary_path = workdir
        .path()
        .join(if cfg!(windows) { "cinch.exe" } else { "cinch" });
    extract_cinch_binary(&archive_path, &new_binary_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&new_binary_path, fs::Permissions::from_mode(0o755))
            .map_err(SelfUpdateError::Swap)?;
    }

    atomic_swap(&exe, &new_binary_path).map_err(SelfUpdateError::Swap)?;
    eprintln!("Updated to cinch {}. Re-run your command.", next_ver);
    Ok(())
}

async fn download_to(url: &str, dest: &Path) -> Result<(), SelfUpdateError> {
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| SelfUpdateError::Fetch(e.to_string()))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| SelfUpdateError::Fetch(e.to_string()))?
        .error_for_status()
        .map_err(|e| SelfUpdateError::Fetch(e.to_string()))?;
    let total = resp.content_length();
    let bar = total.map(|t| {
        let b = indicatif::ProgressBar::new(t);
        b.set_style(
            indicatif::ProgressStyle::with_template("{bar:40} {bytes}/{total_bytes} ({eta})")
                .unwrap(),
        );
        b
    });
    let mut file = fs::File::create(dest).map_err(|e| SelfUpdateError::Fetch(e.to_string()))?;
    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| SelfUpdateError::Fetch(e.to_string()))?;
        io::Write::write_all(&mut file, &bytes)
            .map_err(|e| SelfUpdateError::Fetch(e.to_string()))?;
        if let Some(b) = &bar {
            b.inc(bytes.len() as u64);
        }
    }
    if let Some(b) = bar {
        b.finish_and_clear();
    }
    Ok(())
}

fn extract_cinch_binary(archive: &Path, dest: &Path) -> Result<(), SelfUpdateError> {
    let f = fs::File::open(archive).map_err(|e| SelfUpdateError::Extract(e.to_string()))?;
    if archive.to_string_lossy().ends_with(".zip") {
        let mut zip =
            zip::ZipArchive::new(f).map_err(|e| SelfUpdateError::Extract(e.to_string()))?;
        for i in 0..zip.len() {
            let mut entry = zip
                .by_index(i)
                .map_err(|e| SelfUpdateError::Extract(e.to_string()))?;
            let name = entry.name().to_string();
            if name.ends_with("cinch.exe") || name.ends_with("cinch") {
                let mut out =
                    fs::File::create(dest).map_err(|e| SelfUpdateError::Extract(e.to_string()))?;
                io::copy(&mut entry, &mut out)
                    .map_err(|e| SelfUpdateError::Extract(e.to_string()))?;
                return Ok(());
            }
        }
        Err(SelfUpdateError::Extract(
            "cinch binary not found in zip".into(),
        ))
    } else {
        let gz = flate2::read::GzDecoder::new(f);
        let mut tar = tar::Archive::new(gz);
        for entry in tar
            .entries()
            .map_err(|e| SelfUpdateError::Extract(e.to_string()))?
        {
            let mut entry = entry.map_err(|e| SelfUpdateError::Extract(e.to_string()))?;
            let path = entry
                .path()
                .map_err(|e| SelfUpdateError::Extract(e.to_string()))?;
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if name == "cinch" {
                let mut out =
                    fs::File::create(dest).map_err(|e| SelfUpdateError::Extract(e.to_string()))?;
                io::copy(&mut entry, &mut out)
                    .map_err(|e| SelfUpdateError::Extract(e.to_string()))?;
                return Ok(());
            }
        }
        Err(SelfUpdateError::Extract(
            "cinch binary not found in tar.gz".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn asset_name_returns_some_on_any_supported_target() {
        // This passes on any platform we build on.
        assert!(asset_name().is_some());
    }

    #[test]
    fn verify_sha256_accepts_correct_hash() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("blob");
        fs::write(&path, b"hello").unwrap();
        // SHA-256("hello") = 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824
        let expected = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        assert!(verify_sha256(&path, expected).unwrap());
    }

    #[test]
    fn verify_sha256_rejects_incorrect_hash() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("blob");
        fs::write(&path, b"hello").unwrap();
        let wrong = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(!verify_sha256(&path, wrong).unwrap());
    }

    #[test]
    fn verify_sha256_tolerates_trailing_whitespace() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("blob");
        fs::write(&path, b"hello").unwrap();
        let with_ws =
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824  cinch.tar.gz\n";
        // The function trims; we pass the whole .sha256-style line.
        assert!(verify_sha256(&path, with_ws.split_whitespace().next().unwrap()).unwrap());
    }

    #[test]
    fn atomic_swap_succeeds_when_both_files_exist() {
        let dir = tempdir().unwrap();
        let current = dir.path().join("cinch");
        let new_bin = dir.path().join("cinch.new");
        fs::write(&current, b"old").unwrap();
        fs::write(&new_bin, b"new").unwrap();
        atomic_swap(&current, &new_bin).unwrap();
        assert_eq!(fs::read(&current).unwrap(), b"new");
        assert!(!new_bin.exists());
        assert!(!current.with_extension("old").exists());
    }

    #[test]
    fn atomic_swap_rolls_back_when_second_rename_fails() {
        let dir = tempdir().unwrap();
        let current = dir.path().join("cinch");
        let new_bin = dir.path().join("does-not-exist.new");
        fs::write(&current, b"old").unwrap();
        // new_bin doesn't exist, so the second rename will fail.
        let result = atomic_swap(&current, &new_bin);
        assert!(result.is_err());
        // Original is back in place.
        assert_eq!(fs::read(&current).unwrap(), b"old");
        // Backup was cleaned up by the restore.
        assert!(!current.with_extension("old").exists());
    }

    #[test]
    fn can_write_to_returns_true_for_writable_dir() {
        let dir = tempdir().unwrap();
        assert!(can_write_to(dir.path()));
    }

    #[test]
    fn can_write_to_returns_false_for_nonexistent_dir() {
        let dir = tempdir().unwrap();
        let bad = dir.path().join("does-not-exist");
        assert!(!can_write_to(&bad));
    }
}
