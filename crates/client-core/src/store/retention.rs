//! Per-device retention preferences.

use super::models::RetentionPref;
use super::{Store, StoreError};
use rusqlite::params;

pub fn set_retention(store: &Store, device_id: &str, days: i64) -> Result<(), StoreError> {
    store.with_conn(|conn| {
        conn.execute(
            "INSERT INTO retention_prefs(device_id, days) VALUES(?1, ?2)
             ON CONFLICT(device_id) DO UPDATE SET days = excluded.days",
            params![device_id, days],
        )?;
        Ok(())
    })
}

pub fn list_retention(store: &Store) -> Result<Vec<RetentionPref>, StoreError> {
    store.with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT device_id, days FROM retention_prefs")?;
        let rows: Vec<RetentionPref> = stmt
            .query_map([], |r| {
                Ok(RetentionPref {
                    device_id: r.get(0)?,
                    days: r.get(1)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}
