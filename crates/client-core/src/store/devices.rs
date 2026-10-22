//! Read models for paired devices and the clip-derived source list.

use super::models::{SourceRow, StoredDevice};
use super::{Store, StoreError};

pub fn list_sources(store: &Store) -> Result<Vec<SourceRow>, StoreError> {
    store.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT source, COUNT(*) AS c, MAX(created_at) AS last_seen
             FROM clips GROUP BY source ORDER BY last_seen DESC NULLS LAST",
        )?;
        let rows: Vec<SourceRow> = stmt
            .query_map([], |r| {
                Ok(SourceRow {
                    source: r.get(0)?,
                    clip_count: r.get(1)?,
                    last_seen: r.get(2)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

pub fn list_devices(store: &Store) -> Result<Vec<StoredDevice>, StoreError> {
    store.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, hostname, nickname, source_key, machine_id, public_key,
                    paired_at, last_push_at, online, refreshed_at
             FROM devices ORDER BY last_push_at DESC NULLS LAST",
        )?;
        let rows: Vec<StoredDevice> = stmt
            .query_map([], |r| {
                let hostname: String = r.get(1)?;
                let source_key: Option<String> = r.get(3)?;
                let source_key =
                    source_key.map(|source| crate::machine::clip_source_key(&source, &hostname));
                Ok(StoredDevice {
                    id: r.get(0)?,
                    hostname,
                    nickname: r.get(2)?,
                    source_key,
                    machine_id: r.get(4)?,
                    public_key: r.get(5)?,
                    paired_at: r.get(6)?,
                    last_push_at: r.get(7)?,
                    online: r.get::<_, i64>(8)? != 0,
                    refreshed_at: r.get(9)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_approval_devices_use_clip_sources_without_rewriting_ids() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store.with_conn(|conn| conn.execute(
            "INSERT INTO devices (id, hostname, source_key, refreshed_at) VALUES ('approved', 'atlas', 'approve:approved', 0)",
            [],
        )).unwrap();
        let devices = list_devices(&store).unwrap();
        assert_eq!(devices[0].id, "approved");
        assert_eq!(devices[0].source_key.as_deref(), Some("remote:atlas"));
        let persisted: String = store
            .with_conn(|conn| {
                conn.query_row(
                    "SELECT source_key FROM devices WHERE id = 'approved'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(persisted, "approve:approved");
    }
}
