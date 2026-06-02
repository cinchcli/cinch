//! In-memory list of pending device-code approval requests (remote-login flow).

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use specta::Type;

/// A device-code approval request forwarded from the relay via WebSocket.
/// Derives `specta::Type` so tauri-specta can generate the TypeScript binding.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct PendingDeviceCode {
    pub user_code: String,
    pub hostname: String,
    pub source_region: String,
    /// Unix timestamp (seconds) when the request arrived at the relay.
    pub requested_at: i64,
}

#[cfg(test)]
impl PendingDeviceCode {
    fn sample(code: &str) -> Self {
        Self {
            user_code: code.into(),
            hostname: "dev-box-3".into(),
            source_region: "us-west".into(),
            requested_at: chrono::Utc::now().timestamp(),
        }
    }
}

/// Shared in-memory list of pending device-code approval requests.
/// Follows the same `Arc<Mutex<...>>` type-alias pattern as `AuthStateHandle`.
pub type PendingCodesHandle = Arc<Mutex<Vec<PendingDeviceCode>>>;

/// Push `p` onto the list, silently deduplicating by `user_code`.
pub fn add_pending_code(handle: &PendingCodesHandle, p: PendingDeviceCode) {
    let mut guard = handle.lock().unwrap();
    if guard
        .iter()
        .any(|existing| existing.user_code == p.user_code)
    {
        return;
    }
    guard.push(p);
}

/// Remove the entry whose `user_code` matches `code`. No-op if not found.
pub fn remove_pending_code(handle: &PendingCodesHandle, code: &str) {
    let mut guard = handle.lock().unwrap();
    guard.retain(|p| p.user_code != code);
}

/// Return the number of pending requests.
pub fn pending_count(handle: &PendingCodesHandle) -> usize {
    handle.lock().unwrap().len()
}

/// Return a cloned snapshot of all pending requests.
pub fn pending_codes(handle: &PendingCodesHandle) -> Vec<PendingDeviceCode> {
    handle.lock().unwrap().clone()
}

/// Drop entries whose `requested_at` is older than `ttl` from now.
pub fn sweep_expired(handle: &PendingCodesHandle, ttl: std::time::Duration) {
    let ttl_secs = ttl.as_secs() as i64;
    let now = chrono::Utc::now().timestamp();
    let cutoff = now - ttl_secs;
    let mut guard = handle.lock().unwrap();
    guard.retain(|p| p.requested_at >= cutoff);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_remove() {
        let h: PendingCodesHandle = Arc::new(Mutex::new(Vec::new()));
        add_pending_code(&h, PendingDeviceCode::sample("ABCD-1234"));
        assert_eq!(pending_count(&h), 1);
        remove_pending_code(&h, "ABCD-1234");
        assert_eq!(pending_count(&h), 0);
    }

    #[test]
    fn dedup_same_user_code() {
        let h: PendingCodesHandle = Arc::new(Mutex::new(Vec::new()));
        add_pending_code(&h, PendingDeviceCode::sample("ABCD-1234"));
        add_pending_code(&h, PendingDeviceCode::sample("ABCD-1234"));
        assert_eq!(pending_count(&h), 1);
    }

    #[test]
    fn ttl_sweep_drops_expired() {
        let h: PendingCodesHandle = Arc::new(Mutex::new(Vec::new()));
        let mut p = PendingDeviceCode::sample("ABCD-1234");
        p.requested_at = chrono::Utc::now().timestamp() - 6 * 60; // 6 min ago
        add_pending_code(&h, p);
        sweep_expired(&h, std::time::Duration::from_secs(5 * 60));
        assert_eq!(pending_count(&h), 0);
    }
}
