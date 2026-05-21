use log::info;
use rusqlite::params;

use super::Database;

impl Database {
    /// Delete every clip row with `created_at < cutoff` and cascade-delete
    /// its media file. Returns the number of rows deleted.
    ///
    /// Uses rusqlite `params!` parameter binding — string formatting of the
    /// cutoff is forbidden (Tampering / SQLi; see plan 01-02 threat model).
    pub fn purge_before(&self, cutoff: i64) -> Result<usize, String> {
        let conn = self.conn.lock().unwrap();

        // 1. Collect media paths of soon-to-be-deleted rows.
        let mut stmt = conn
            .prepare(
                "SELECT media_path FROM clips WHERE created_at < ?1 AND is_pinned = 0 \
                 AND media_path IS NOT NULL AND media_path != ''",
            )
            .map_err(|e| format!("prepare failed: {}", e))?;
        let media_paths: Vec<String> = stmt
            .query_map(params![cutoff], |row| row.get(0))
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        // 2. DELETE (parameterised). Pinned clips are exempt from retention purge.
        let deleted = conn
            .execute(
                "DELETE FROM clips WHERE created_at < ?1 AND is_pinned = 0",
                params![cutoff],
            )
            .map_err(|e| format!("purge failed: {}", e))?;

        // 3. Cascade-delete media files (same idiom as cleanup_expired).
        if !media_paths.is_empty() {
            let media_dir = dirs::data_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap().join(".local/share"))
                .join("com.cinch.app");
            for mp in &media_paths {
                let _ = std::fs::remove_file(media_dir.join(mp));
            }
        }

        if deleted > 0 {
            info!(
                "retention: purged {} clips older than cutoff {}",
                deleted, cutoff
            );
        }
        Ok(deleted)
    }

    /// Count clips with `created_at < cutoff` without deleting.
    /// Used to populate the retroactive-purge confirmation dialog.
    /// Called from `commands::clips::preview_retention_change` (plan 01-06).
    pub fn count_clips_before(&self, cutoff: i64) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM clips WHERE created_at < ?1",
            params![cutoff],
            |row| row.get(0),
        )
        .map_err(|e| format!("count_clips_before failed: {}", e))
    }

    /// Delete every clip row and cascade-delete every media file.
    /// Returns the number of rows deleted as `i64`.
    /// Called from `commands::clips::clear_local_history` (plan 01-06).
    pub fn clear_all_clips(&self) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();

        // 1. Collect all media paths.
        let mut stmt = conn
            .prepare(
                "SELECT media_path FROM clips WHERE media_path IS NOT NULL AND media_path != ''",
            )
            .map_err(|e| format!("prepare failed: {}", e))?;
        let media_paths: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        // 2. Unconditional DELETE.
        let deleted = conn
            .execute("DELETE FROM clips", [])
            .map_err(|e| format!("clear failed: {}", e))? as i64;

        // 3. Cascade-delete media files.
        if !media_paths.is_empty() {
            let media_dir = dirs::data_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap().join(".local/share"))
                .join("com.cinch.app");
            for mp in &media_paths {
                let _ = std::fs::remove_file(media_dir.join(mp));
            }
        }

        if deleted > 0 {
            info!("clear_local_history: deleted {} clips", deleted);
        }
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::{make_clip, test_db};

    #[test]
    fn purge_before_deletes_old_rows() {
        let db = test_db();
        let now = chrono::Utc::now().timestamp();
        let thirty_days = 30 * 86_400_i64;

        // Row "old" is older than 30 days; should be purged.
        let mut old_clip = make_clip("old", "old content", "remote:prod", "text");
        old_clip.created_at = now - 100 * 86_400;
        db.insert_clip(&old_clip).unwrap();

        // Row "new" is 1 day old; should survive.
        let mut new_clip = make_clip("new", "new content", "remote:prod", "text");
        new_clip.created_at = now - 86_400;
        db.insert_clip(&new_clip).unwrap();

        let deleted = db.purge_before(now - thirty_days).unwrap();
        assert_eq!(deleted, 1, "exactly one row should be purged");

        // Verify new row survived.
        let count = db.clip_count().unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn purge_before_cascades_media() {
        let db = test_db();
        let now = chrono::Utc::now().timestamp();
        let thirty_days = 30 * 86_400_i64;

        // Text row (no media_path) older than 30 days.
        let mut text_clip = make_clip("txt-old", "text only", "local", "text");
        text_clip.created_at = now - 100 * 86_400;
        db.insert_clip(&text_clip).unwrap();

        // Image row (with media_path) older than 30 days.
        // Use a unique filename under a temp-like subdirectory so test doesn't
        // clobber a real media dir. std::fs::remove_file is best-effort (matches
        // cleanup_expired pattern), so the test passes even if the file does
        // not exist on disk.
        let media_rel = format!("cinch-test-media-{}-{}.png", std::process::id(), now);
        let mut img_clip = make_clip("img-old", "", "local", "image");
        img_clip.media_path = Some(media_rel.clone());
        img_clip.created_at = now - 100 * 86_400;
        db.insert_clip(&img_clip).unwrap();

        // Pre-create the media file so we can verify cascade removal.
        let media_dir = dirs::data_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap().join(".local/share"))
            .join("com.cinch.app");
        let _ = std::fs::create_dir_all(&media_dir);
        let full_path = media_dir.join(&media_rel);
        let _ = std::fs::write(&full_path, b"fake png bytes");
        let existed_before = full_path.exists();

        let deleted = db.purge_before(now - thirty_days).unwrap();
        assert_eq!(deleted, 2, "both rows should be purged");
        assert_eq!(db.clip_count().unwrap(), 0);

        // If we successfully created the file above, verify cascade removed it.
        if existed_before {
            assert!(
                !full_path.exists(),
                "media file should be cascade-deleted: {}",
                full_path.display()
            );
        }
        // Defensive cleanup in case the assert above was skipped.
        let _ = std::fs::remove_file(&full_path);
    }

    #[test]
    fn count_clips_before_returns_correct_count() {
        let db = test_db();
        let now = chrono::Utc::now().timestamp();

        // Three rows at 10d, 40d, 80d old.
        for (id, days_ago) in [("a", 10_i64), ("b", 40), ("c", 80)] {
            let mut c = make_clip(id, id, "remote:prod", "text");
            c.created_at = now - days_ago * 86_400;
            db.insert_clip(&c).unwrap();
        }

        let count = db.count_clips_before(now - 30 * 86_400).unwrap();
        assert_eq!(count, 2, "rows at 40d and 80d should be counted");
    }

    #[test]
    fn count_clips_before_boundary() {
        let db = test_db();
        let now = chrono::Utc::now().timestamp();
        let mut c = make_clip("boundary", "at cutoff", "remote:prod", "text");
        c.created_at = now - 30 * 86_400; // exactly at cutoff — NOT strictly less than
        db.insert_clip(&c).unwrap();

        let count = db.count_clips_before(now - 30 * 86_400).unwrap();
        assert_eq!(count, 0, "< is strict; rows AT cutoff should not count");
    }

    #[test]
    fn clear_all_clips_removes_everything() {
        let db = test_db();
        for i in 0..5 {
            let c = make_clip(&format!("id-{}", i), "content", "remote:prod", "text");
            db.insert_clip(&c).unwrap();
        }
        assert_eq!(db.clip_count().unwrap(), 5);

        let deleted = db.clear_all_clips().unwrap();
        assert_eq!(deleted, 5);
        assert_eq!(db.clip_count().unwrap(), 0);
    }
}
