use fs2::FileExt;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Copy, Serialize)]
pub enum LockKind {
    Desktop,
    Cli,
}

#[derive(Serialize)]
struct LockBody<'a> {
    pid: u32,
    kind: &'a str,
    started_at: String,
}

pub struct Lockfile {
    file: File, // dropped → OS releases the lock
}

impl Lockfile {
    /// Acquire an exclusive advisory lock at `path` without blocking.
    /// Returns `Ok(Some(Lockfile))` if acquired, `Ok(None)` if the lock is
    /// already held by another process.
    pub fn try_acquire(path: &Path, kind: LockKind) -> std::io::Result<Option<Self>> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        match file.try_lock_exclusive() {
            Ok(()) => {
                let body = LockBody {
                    pid: std::process::id(),
                    kind: match kind {
                        LockKind::Desktop => "desktop",
                        LockKind::Cli => "cli",
                    },
                    started_at: chrono::Utc::now().to_rfc3339(),
                };
                let mut f = &file;
                let _ = f.set_len(0);
                let _ = writeln!(f, "{}", serde_json::to_string(&body).unwrap_or_default());
                Ok(Some(Self { file }))
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || matches!(e.raw_os_error(), Some(11) | Some(35)) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// Probe whether the lock is currently free, **without holding it on success**.
    /// Useful for readers that want to skip backfill when a writer is active.
    pub fn is_held_by_other(path: &Path) -> std::io::Result<bool> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let f = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        match f.try_lock_exclusive() {
            Ok(()) => {
                // Disambiguate from std's File::unlock (stable since 1.89) so
                // builds on older toolchains continue to use fs2's trait impl.
                let _ = FileExt::unlock(&f);
                Ok(false)
            }
            Err(_) => Ok(true),
        }
    }
}

impl Drop for Lockfile {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

/// Convenience: poll the lock until acquired or `deadline` elapses.
pub async fn acquire_blocking(
    path: &Path,
    kind: LockKind,
    poll: Duration,
    deadline: std::time::Instant,
) -> std::io::Result<Option<Lockfile>> {
    loop {
        match Lockfile::try_acquire(path, kind)? {
            Some(l) => return Ok(Some(l)),
            None if std::time::Instant::now() >= deadline => return Ok(None),
            None => tokio::time::sleep(poll).await,
        }
    }
}
