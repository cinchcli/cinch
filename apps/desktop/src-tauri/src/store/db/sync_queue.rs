use log::info;
use rusqlite::params;

use super::Database;
#[cfg(test)]
use crate::store::models::LocalClip;

impl Database {
    /// Returns all clips with `synced = false`, ordered by `created_at ASC` (oldest first).
    /// Used by the offline push queue to flush pending clips on reconnect.
    #[cfg(test)]
    pub fn list_unsynced_clips(&self) -> Result<Vec<LocalClip>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, content, content_type, source, label, byte_size, media_path, created_at, synced, is_pinned, pin_note, received_at
                 FROM clips WHERE synced = FALSE ORDER BY created_at ASC",
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
                    is_pinned: row.get::<_, i32>(10).unwrap_or(0) != 0,
                    pin_note: row.get(11)?,
                    received_at: row.get::<_, i64>(12).unwrap_or(0),
                })
            })
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(clips)
    }

    /// Mark a clip as synced after successful push to relay.
    #[cfg(test)]
    pub fn mark_synced(&self, clip_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clips SET synced = TRUE WHERE id = ?1",
            params![clip_id],
        )
        .map_err(|e| format!("mark_synced failed: {}", e))?;
        Ok(())
    }

    /// Enforce the offline queue cap by dropping the oldest unsynced clips
    /// when the count exceeds `max_unsynced`. Returns the number of clips dropped.
    /// Mitigates T-04-07 (DoS via unbounded DB growth during extended offline).
    ///
    /// Currently exercised only by the in-file tests — production callers were
    /// removed when the clipboard monitor moved to `LocalPusher`. A real
    /// offline-queue replacement on the shared store is a follow-up.
    #[allow(dead_code)]
    pub fn enforce_offline_cap(&self, max_unsynced: usize) -> Result<usize, String> {
        let conn = self.conn.lock().unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clips WHERE synced = FALSE",
                [],
                |row| row.get(0),
            )
            .map_err(|e| format!("count unsynced failed: {}", e))?;

        let count = count as usize;
        if count <= max_unsynced {
            return Ok(0);
        }

        let excess = count - max_unsynced;
        conn.execute(
            "DELETE FROM clips WHERE id IN (
                SELECT id FROM clips WHERE synced = FALSE
                ORDER BY created_at ASC LIMIT ?1
            )",
            params![excess as i64],
        )
        .map_err(|e| format!("enforce_offline_cap failed: {}", e))?;

        info!(
            "offline queue cap: dropped {} oldest unsynced clips (cap={})",
            excess, max_unsynced
        );
        Ok(excess)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::{make_clip, test_db};

    #[test]
    fn test_list_unsynced_clips_returns_only_unsynced() {
        let db = test_db();
        let mut synced_clip = make_clip("s1", "synced", "local", "text");
        synced_clip.synced = true;
        db.insert_clip(&synced_clip).unwrap();

        let mut unsynced_clip = make_clip("u1", "unsynced", "local", "text");
        unsynced_clip.synced = false;
        unsynced_clip.created_at = chrono::Utc::now().timestamp() + 1;
        db.insert_clip(&unsynced_clip).unwrap();

        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 1);
        assert_eq!(unsynced[0].id, "u1");
    }

    #[test]
    fn test_list_unsynced_clips_ordered_by_created_at_asc() {
        let db = test_db();
        let now = chrono::Utc::now().timestamp();

        let mut clip_old = make_clip("u-old", "old", "local", "text");
        clip_old.synced = false;
        clip_old.created_at = now - 100;
        db.insert_clip(&clip_old).unwrap();

        let mut clip_new = make_clip("u-new", "new", "local", "text");
        clip_new.synced = false;
        clip_new.created_at = now;
        db.insert_clip(&clip_new).unwrap();

        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 2);
        assert_eq!(unsynced[0].id, "u-old", "oldest first");
        assert_eq!(unsynced[1].id, "u-new", "newest last");
    }

    #[test]
    fn test_list_unsynced_clips_empty_when_all_synced() {
        let db = test_db();
        db.insert_clip(&make_clip("s1", "synced", "local", "text"))
            .unwrap();

        let unsynced = db.list_unsynced_clips().unwrap();
        assert!(unsynced.is_empty());
    }

    #[test]
    fn test_mark_synced() {
        let db = test_db();
        let mut clip = make_clip("u1", "unsynced", "local", "text");
        clip.synced = false;
        db.insert_clip(&clip).unwrap();

        // Verify it starts unsynced
        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 1);

        // Mark synced
        db.mark_synced("u1").unwrap();

        // Verify it's now synced
        let unsynced = db.list_unsynced_clips().unwrap();
        assert!(unsynced.is_empty());

        let all = db.list_clips(None, None, 50).unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].synced);
    }

    #[test]
    fn test_enforce_offline_cap_drops_oldest() {
        let db = test_db();
        let now = chrono::Utc::now().timestamp();

        // Insert 5 unsynced clips
        for i in 0..5 {
            let mut clip = make_clip(
                &format!("u{}", i),
                &format!("content {}", i),
                "local",
                "text",
            );
            clip.synced = false;
            clip.created_at = now + i as i64; // ascending order
            db.insert_clip(&clip).unwrap();
        }

        // Cap at 3: should drop 2 oldest (u0, u1)
        let dropped = db.enforce_offline_cap(3).unwrap();
        assert_eq!(dropped, 2);

        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 3);
        assert_eq!(unsynced[0].id, "u2");
        assert_eq!(unsynced[1].id, "u3");
        assert_eq!(unsynced[2].id, "u4");
    }

    #[test]
    fn test_enforce_offline_cap_noop_under_cap() {
        let db = test_db();

        let mut clip = make_clip("u1", "content", "local", "text");
        clip.synced = false;
        db.insert_clip(&clip).unwrap();

        let dropped = db.enforce_offline_cap(500).unwrap();
        assert_eq!(dropped, 0);

        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 1);
    }
}
