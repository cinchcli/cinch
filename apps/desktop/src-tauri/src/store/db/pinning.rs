#[cfg(test)]
use rusqlite::params;

use super::Database;
#[cfg(test)]
use crate::store::models::LocalClip;

impl Database {
    #[cfg(test)]
    pub fn list_pinned_clips(&self) -> Result<Vec<LocalClip>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, content, content_type, source, label, byte_size, media_path, created_at, synced, is_pinned, pin_note, received_at
                 FROM clips WHERE is_pinned = 1 ORDER BY created_at DESC",
            )
            .map_err(|e| format!("prepare failed: {}", e))?;

        let clips = stmt
            .query_map([], |row| {
                Ok(LocalClip {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    content: row.get(2)?,
                    content_type: row.get(3)?,
                    source: row.get(4)?,
                    label: row.get(5)?,
                    byte_size: row.get(6)?,
                    media_path: row.get(7)?,
                    created_at: row.get(8)?,
                    synced: row.get::<_, bool>(9).unwrap_or(true),
                    is_pinned: true,
                    pin_note: row.get(11)?,
                    received_at: row.get::<_, i64>(12).unwrap_or(0),
                })
            })
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(clips)
    }

    #[cfg(test)]
    pub fn pin_clip(&self, id: &str, note: Option<&str>) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clips SET is_pinned = 1, pin_note = ?2 WHERE id = ?1",
            params![id, note],
        )
        .map_err(|e| format!("pin_clip failed: {}", e))?;
        Ok(())
    }
}
