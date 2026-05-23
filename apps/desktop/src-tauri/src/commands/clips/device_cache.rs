//! Process-local TTL cache for `list_devices`.
//!
//! The desktop's `DevicesPanel` polls `list_devices` every 5 seconds. Without
//! a cache that becomes an unconditional `GET /devices` round trip to the
//! relay every 5 seconds. A 30-second TTL turns ~6 of every 7 polls into a
//! local memory read while still surfacing relay-side changes within half a
//! minute. Mutations (`set_device_nickname`, `revoke_device`, `sign_out`,
//! `pair_with_token`, `pair_via_ssh`) explicitly invalidate the cache so
//! the following read sees the new state immediately.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::protocol::DeviceInfo;

pub const DEVICE_CACHE_TTL: Duration = Duration::from_secs(30);

pub type DeviceCacheHandle = Arc<DeviceCache>;

pub struct DeviceCache {
    inner: Mutex<Option<Entry>>,
}

struct Entry {
    inserted_at: Instant,
    devices: Vec<DeviceInfo>,
}

impl DeviceCache {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
        }
    }

    /// Returns a cloned `Vec<DeviceInfo>` if a fresh entry is present, else `None`.
    /// Freshness is decided against `DEVICE_CACHE_TTL` using `Instant::now()`.
    pub fn get(&self) -> Option<Vec<DeviceInfo>> {
        let guard = self.inner.lock().expect("device cache poisoned");
        let entry = guard.as_ref()?;
        if entry.inserted_at.elapsed() < DEVICE_CACHE_TTL {
            Some(entry.devices.clone())
        } else {
            None
        }
    }

    /// Overwrites the cache with `devices`, stamped at `Instant::now()`.
    pub fn insert(&self, devices: Vec<DeviceInfo>) {
        let mut guard = self.inner.lock().expect("device cache poisoned");
        *guard = Some(Entry {
            inserted_at: Instant::now(),
            devices,
        });
    }

    /// Drops the cached entry. Next `get()` returns `None`.
    pub fn invalidate(&self) {
        let mut guard = self.inner.lock().expect("device cache poisoned");
        *guard = None;
    }

    #[cfg(test)]
    fn insert_with_timestamp(&self, devices: Vec<DeviceInfo>, inserted_at: Instant) {
        let mut guard = self.inner.lock().expect("device cache poisoned");
        *guard = Some(Entry {
            inserted_at,
            devices,
        });
    }
}

impl Default for DeviceCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> DeviceInfo {
        DeviceInfo {
            id: id.to_string(),
            hostname: "host".to_string(),
            source_key: format!("remote:{}", id),
            clip_count: 0,
            paired_at: String::new(),
            online: false,
            nickname: String::new(),
            public_key: String::new(),
            public_key_fingerprint: String::new(),
            last_push_at: None,
            machine_id: None,
            client_version: None,
            client_type: None,
            client_version_at: None,
        }
    }

    #[test]
    fn empty_cache_returns_none() {
        let cache = DeviceCache::new();
        assert!(cache.get().is_none());
    }

    #[test]
    fn fresh_insert_is_returned() {
        let cache = DeviceCache::new();
        cache.insert(vec![sample("a")]);
        let got = cache.get().expect("fresh entry");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id.as_str(), "a");
    }

    #[test]
    fn invalidate_drops_entry() {
        let cache = DeviceCache::new();
        cache.insert(vec![sample("a")]);
        cache.invalidate();
        assert!(cache.get().is_none());
    }

    #[test]
    fn expired_entry_returns_none() {
        let cache = DeviceCache::new();
        let past = Instant::now() - DEVICE_CACHE_TTL - Duration::from_secs(1);
        cache.insert_with_timestamp(vec![sample("a")], past);
        assert!(cache.get().is_none());
    }

    #[test]
    fn second_insert_overwrites_first() {
        let cache = DeviceCache::new();
        cache.insert(vec![sample("a")]);
        cache.insert(vec![sample("b"), sample("c")]);
        let got = cache.get().expect("fresh entry");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].id.as_str(), "b");
    }
}
