//! `ClipboardService` — the single public entry point for clipboard access.
//!
//! Wraps a platform backend behind a `Mutex` so concurrent callers serialize
//! cleanly (preventing arboard's Windows `ClipboardOccupied` and Linux
//! ownership issues once those backends land). Tracks self-writes internally
//! so the monitor loop doesn't re-capture clips Cinch itself just wrote.

use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};

use super::backend::{self, Backend, ClipboardError, PollContent, PollSnapshot};

pub struct ClipboardService {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    backend: Box<dyn Backend + Send>,
    last_self_write: Option<SelfWriteRecord>,
}

struct SelfWriteRecord {
    content_hash: [u8; 32],
    token_at_write: Option<u64>,
}

impl ClipboardService {
    pub fn new_platform_default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                backend: backend::platform_default(),
                last_self_write: None,
            })),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_backend(b: Box<dyn Backend + Send>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                backend: b,
                last_self_write: None,
            })),
        }
    }

    pub fn poll_snapshot(&self) -> Result<PollSnapshot, ClipboardError> {
        let mut inner = self.inner.lock().expect("clipboard mutex poisoned");
        inner.backend.read_snapshot()
    }

    pub fn write_text(&self, content: &str) -> Result<(), ClipboardError> {
        let hash = content_hash(content.as_bytes());
        let mut inner = self.inner.lock().expect("clipboard mutex poisoned");
        inner.backend.write_text(content)?;
        let token = inner.backend.read_snapshot().ok().and_then(|s| s.token);
        inner.last_self_write = Some(SelfWriteRecord {
            content_hash: hash,
            token_at_write: token,
        });
        Ok(())
    }

    /// Write raw PNG bytes to the clipboard (no file round-trip). Records the
    /// write so the monitor loop's self-write dedup skips the echo.
    pub fn write_image_png_bytes(&self, png_bytes: &[u8]) -> Result<(), ClipboardError> {
        let hash = content_hash(png_bytes);
        let mut inner = self.inner.lock().expect("clipboard mutex poisoned");
        inner.backend.write_image_png(png_bytes)?;
        let token = inner.backend.read_snapshot().ok().and_then(|s| s.token);
        inner.last_self_write = Some(SelfWriteRecord {
            content_hash: hash,
            token_at_write: token,
        });
        Ok(())
    }

    /// Is `snapshot` the echo of our last write?
    pub fn is_self_write(&self, snapshot: &PollSnapshot) -> bool {
        let inner = self.inner.lock().expect("clipboard mutex poisoned");
        let Some(ref rec) = inner.last_self_write else {
            return false;
        };

        if let (Some(snap_tok), Some(write_tok)) = (snapshot.token, rec.token_at_write) {
            if snap_tok == write_tok {
                return true;
            }
        }

        let snap_hash = match &snapshot.content {
            PollContent::Text(t) => Some(content_hash(t.as_bytes())),
            PollContent::ImagePng(bytes) => Some(content_hash(bytes)),
            _ => None,
        };
        snap_hash == Some(rec.content_hash)
    }

    /// Current backend change token. Backwards-compat for callers that used
    /// `get_pasteboard_change_count()` + `has_clipboard_changed_since()`.
    pub fn token(&self) -> Option<u64> {
        let mut inner = self.inner.lock().expect("clipboard mutex poisoned");
        inner.backend.read_snapshot().ok().and_then(|s| s.token)
    }

    pub fn default_excluded_apps(&self) -> Vec<String> {
        let inner = self.inner.lock().expect("clipboard mutex poisoned");
        inner.backend.default_excluded_apps()
    }
}

fn content_hash(bytes: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::backend::Backend;

    struct ScriptedBackend {
        pub token: u64,
        pub next_content: PollContent,
        pub last_written_text: Option<String>,
    }

    impl Backend for ScriptedBackend {
        fn read_snapshot(&mut self) -> Result<PollSnapshot, ClipboardError> {
            Ok(PollSnapshot {
                token: Some(self.token),
                content: self.next_content.clone(),
                app_identity: None,
            })
        }
        fn write_text(&mut self, content: &str) -> Result<(), ClipboardError> {
            self.last_written_text = Some(content.to_string());
            self.token += 1;
            self.next_content = PollContent::Text(content.to_string());
            Ok(())
        }
        fn write_image_png(&mut self, _: &[u8]) -> Result<(), ClipboardError> {
            self.token += 1;
            Ok(())
        }
        fn default_excluded_apps(&self) -> Vec<String> {
            vec!["com.test.excluded".into()]
        }
    }

    #[test]
    fn write_then_observe_same_content_is_self_write() {
        let backend = Box::new(ScriptedBackend {
            token: 10,
            next_content: PollContent::Empty,
            last_written_text: None,
        });
        let svc = ClipboardService::new_with_backend(backend);
        svc.write_text("hello").unwrap();
        let snap = svc.poll_snapshot().unwrap();
        assert!(svc.is_self_write(&snap));
    }

    #[test]
    fn external_write_is_not_self_write() {
        let backend = Box::new(ScriptedBackend {
            token: 10,
            next_content: PollContent::Text("external".into()),
            last_written_text: None,
        });
        let svc = ClipboardService::new_with_backend(backend);
        let snap = svc.poll_snapshot().unwrap();
        assert!(!svc.is_self_write(&snap));
    }

    #[test]
    fn default_excluded_apps_from_backend() {
        let backend = Box::new(ScriptedBackend {
            token: 0,
            next_content: PollContent::Empty,
            last_written_text: None,
        });
        let svc = ClipboardService::new_with_backend(backend);
        assert_eq!(svc.default_excluded_apps(), vec!["com.test.excluded"]);
    }

    #[test]
    fn write_image_png_bytes_reaches_backend() {
        let backend = Box::new(ScriptedBackend {
            token: 1,
            next_content: PollContent::Empty,
            last_written_text: None,
        });
        let svc = ClipboardService::new_with_backend(backend);
        let png = [0x89u8, 0x50, 0x4E, 0x47];
        svc.write_image_png_bytes(&png).unwrap();
        // Recorded as a self-write so the monitor won't re-capture it.
        let snap = PollSnapshot {
            token: Some(2),
            content: PollContent::ImagePng(png.to_vec()),
            app_identity: None,
        };
        assert!(svc.is_self_write(&snap));
    }
}
