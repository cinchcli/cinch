use rusqlite::params;

use super::Database;
#[cfg(test)]
use super::SourceInfo;
use crate::store::models::LocalClip;

impl Database {
    // Legacy clip-row writers — production callers were removed when the
    // clipboard monitor migrated to client_core::sync::LocalPusher. Kept for
    // the in-file test suite that still exercises the legacy schema, and as a
    // safety net for any future one-shot migration code.
    #[allow(dead_code)]
    pub fn insert_clip(&self, clip: &LocalClip) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO clips (id, user_id, content, content_type, source, label, byte_size, media_path, created_at, synced, is_pinned, pin_note, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET
                 content      = excluded.content,
                 content_type = excluded.content_type,
                 source       = excluded.source,
                 label        = excluded.label,
                 byte_size    = excluded.byte_size,
                 created_at   = excluded.created_at,
                 media_path   = excluded.media_path,
                 received_at  = excluded.received_at",
            params![
                clip.id,
                clip.user_id,
                clip.content,
                clip.content_type,
                clip.source,
                clip.label,
                clip.byte_size,
                clip.media_path,
                clip.created_at,
                clip.synced,
                clip.is_pinned as i32,
                clip.pin_note,
                clip.received_at,
            ],
        )
        .map_err(|e| format!("insert failed: {}", e))?;
        Ok(())
    }

    #[cfg(test)]
    pub fn list_clips(
        &self,
        source_filter: Option<&str>,
        type_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<LocalClip>, String> {
        let conn = self.conn.lock().unwrap();

        let mut sql = String::from(
            "SELECT id, user_id, content, content_type, source, label, byte_size, media_path, created_at, synced, is_pinned, pin_note, received_at
             FROM clips WHERE 1=1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(source) = source_filter {
            sql.push_str(" AND source = ?");
            param_values.push(Box::new(source.to_string()));
        }
        if let Some(ctype) = type_filter {
            sql.push_str(" AND content_type = ?");
            param_values.push(Box::new(ctype.to_string()));
        }

        sql.push_str(" ORDER BY received_at DESC, created_at DESC LIMIT ?");
        param_values.push(Box::new(limit));

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| format!("prepare failed: {}", e))?;

        let clips = stmt
            .query_map(params_refs.as_slice(), |row| {
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

    #[cfg(test)]
    pub fn search_clips(&self, query: &str, limit: i64) -> Result<Vec<LocalClip>, String> {
        if query.trim().is_empty() {
            return self.list_clips(None, None, limit);
        }

        let conn = self.conn.lock().unwrap();
        let like_pattern = format!("%{}%", query);
        let mut stmt = conn
            .prepare(
                "SELECT c.id, c.user_id, c.content, c.content_type, c.source, c.label, c.byte_size, c.media_path, c.created_at, c.synced, c.is_pinned, c.pin_note, c.received_at
                 FROM clips c
                 JOIN clips_fts f ON c.rowid = f.rowid
                 WHERE clips_fts MATCH ?1
                 UNION
                 SELECT c.id, c.user_id, c.content, c.content_type, c.source, c.label, c.byte_size, c.media_path, c.created_at, c.synced, c.is_pinned, c.pin_note, c.received_at
                 FROM clips c
                 WHERE c.is_pinned = 1 AND c.pin_note LIKE ?2
                 ORDER BY created_at DESC
                 LIMIT ?3",
            )
            .map_err(|e| format!("prepare failed: {}", e))?;

        let clips = stmt
            .query_map(params![query, like_pattern, limit], |row| {
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
            .map_err(|e| format!("search failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(clips)
    }

    #[cfg(test)]
    pub fn get_sources(&self) -> Result<Vec<SourceInfo>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT source, COUNT(*) as count, MAX(created_at) as last_seen
                 FROM clips
                 GROUP BY source
                 ORDER BY last_seen DESC",
            )
            .map_err(|e| format!("prepare failed: {}", e))?;

        let sources = stmt
            .query_map([], |row| {
                Ok(SourceInfo {
                    source: row.get(0)?,
                    clip_count: row.get(1)?,
                    last_seen: row.get(2)?,
                })
            })
            .map_err(|e| format!("query failed: {}", e))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(sources)
    }

    #[cfg(test)]
    pub fn delete_clip(&self, id: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();

        // Check for media file to cascade-delete
        let media_path: Option<String> = conn
            .query_row(
                "SELECT media_path FROM clips WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .ok();

        conn.execute("DELETE FROM clips WHERE id = ?1", params![id])
            .map_err(|e| format!("delete failed: {}", e))?;

        // Delete media file if present
        if let Some(Some(mp)) = media_path.map(|p| if p.is_empty() { None } else { Some(p) }) {
            let media_dir = dirs::data_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap().join(".local/share"))
                .join("com.cinch.app");
            let full_path = media_dir.join(&mp);
            let _ = std::fs::remove_file(full_path);
        }

        Ok(())
    }

    #[cfg(test)]
    pub fn clip_count(&self) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM clips", [], |row| row.get(0))
            .map_err(|e| format!("count failed: {}", e))
    }

    pub fn mark_clip_copied(&self, id: &str, copied_at: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE clips SET received_at = ?2 WHERE id = ?1",
            params![id, copied_at],
        )
        .map_err(|e| format!("mark_clip_copied failed: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::{make_clip, test_db};

    #[test]
    fn test_insert_and_list() {
        let db = test_db();
        let clip = make_clip("c1", "hello world", "remote:prod", "text");
        db.insert_clip(&clip).unwrap();

        let clips = db.list_clips(None, None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].id, "c1");
        assert_eq!(clips[0].content, "hello world");
    }

    #[test]
    fn test_source_filter() {
        let db = test_db();
        db.insert_clip(&make_clip("c1", "from prod", "remote:prod", "text"))
            .unwrap();
        db.insert_clip(&make_clip("c2", "from staging", "remote:staging", "text"))
            .unwrap();

        let clips = db.list_clips(Some("remote:prod"), None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].source, "remote:prod");
    }

    #[test]
    fn test_fts_search() {
        let db = test_db();
        db.insert_clip(&make_clip(
            "c1",
            "connection refused error",
            "remote:prod",
            "error",
        ))
        .unwrap();
        db.insert_clip(&make_clip("c2", "hello world", "remote:prod", "text"))
            .unwrap();

        let results = db.search_clips("connection", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "c1");
    }

    #[test]
    fn test_get_sources() {
        let db = test_db();
        db.insert_clip(&make_clip("c1", "a", "remote:prod", "text"))
            .unwrap();
        db.insert_clip(&make_clip("c2", "b", "remote:prod", "text"))
            .unwrap();
        db.insert_clip(&make_clip("c3", "c", "remote:staging", "text"))
            .unwrap();

        let sources = db.get_sources().unwrap();
        assert_eq!(sources.len(), 2);
    }

    #[test]
    fn test_delete() {
        let db = test_db();
        db.insert_clip(&make_clip("c1", "hello", "remote:prod", "text"))
            .unwrap();
        db.delete_clip("c1").unwrap();
        let clips = db.list_clips(None, None, 50).unwrap();
        assert!(clips.is_empty());
    }

    #[test]
    fn test_mark_clip_copied_updates_received_at_for_local_recency_without_changing_created_at() {
        let db = test_db();
        let mut old = make_clip("old", "old content", "remote:prod", "text");
        old.created_at = 100;
        old.received_at = 100;
        db.insert_clip(&old).unwrap();

        let mut new = make_clip("new", "new content", "remote:prod", "text");
        new.created_at = 200;
        new.received_at = 200;
        db.insert_clip(&new).unwrap();

        db.mark_clip_copied("old", 300).unwrap();

        let clips = db.list_clips(None, None, 50).unwrap();
        assert_eq!(clips[0].id, "old");
        assert_eq!(clips[0].created_at, 100);
        assert_eq!(clips[0].received_at, 300);
    }

    #[test]
    fn test_fts5_skips_empty_content() {
        let db = test_db();
        // Insert a clip with empty content (simulates future image clip)
        let mut clip = make_clip("img1", "", "local", "image");
        clip.byte_size = 0;
        db.insert_clip(&clip).unwrap();

        // FTS5 search should return no results for empty content
        let results = db.search_clips("", 50).unwrap();
        // Empty query returns all clips via list_clips fallback
        assert_eq!(results.len(), 1);

        // Actual FTS5 search should not find the empty clip
        let results = db.search_clips("anything", 50).unwrap();
        assert_eq!(results.len(), 0);
    }

    #[test]
    fn test_local_source_clip() {
        let db = test_db();
        let clip = make_clip("l1", "local text", "local", "text");
        db.insert_clip(&clip).unwrap();

        // Should be findable via source filter
        let clips = db.list_clips(Some("local"), None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].source, "local");

        // Should be searchable
        let results = db.search_clips("local text", 50).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_insert_image_clip_with_media_path() {
        let db = test_db();
        let mut clip = make_clip("img1", "", "local", "image");
        clip.media_path = Some("media/img1.png".to_string());
        clip.byte_size = 1024;
        db.insert_clip(&clip).unwrap();

        let clips = db.list_clips(None, Some("image"), 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].media_path, Some("media/img1.png".to_string()));
        assert_eq!(clips[0].byte_size, 1024);
        assert_eq!(clips[0].content, "");
    }

    #[test]
    fn test_search_does_not_return_image_clips() {
        let db = test_db();
        // Insert text clip
        db.insert_clip(&make_clip("t1", "searchable text", "local", "text"))
            .unwrap();
        // Insert image clip (empty content)
        let mut img = make_clip("img1", "", "local", "image");
        img.media_path = Some("media/img1.png".to_string());
        db.insert_clip(&img).unwrap();

        // Text search should only find the text clip
        let results = db.search_clips("searchable", 50).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "t1");
    }

    #[test]
    fn test_mixed_text_and_image_clips() {
        let db = test_db();
        db.insert_clip(&make_clip("t1", "hello", "remote:prod", "text"))
            .unwrap();

        let mut img = make_clip("img1", "", "local", "image");
        img.media_path = Some("media/img1.png".to_string());
        db.insert_clip(&img).unwrap();

        db.insert_clip(&make_clip("t2", "world", "local", "text"))
            .unwrap();

        // All clips
        let all = db.list_clips(None, None, 50).unwrap();
        assert_eq!(all.len(), 3);

        // Filter by image
        let images = db.list_clips(None, Some("image"), 50).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].id, "img1");

        // Filter by text
        let texts = db.list_clips(None, Some("text"), 50).unwrap();
        assert_eq!(texts.len(), 2);
    }

    #[test]
    fn test_insert_clip_synced_true() {
        let db = test_db();
        let clip = make_clip("s1", "synced content", "local", "text");
        assert!(clip.synced);
        db.insert_clip(&clip).unwrap();

        let clips = db.list_clips(None, None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert!(
            clips[0].synced,
            "clip inserted with synced=true should read back as true"
        );
    }

    #[test]
    fn test_insert_clip_synced_false() {
        let db = test_db();
        let mut clip = make_clip("s2", "unsynced content", "local", "text");
        clip.synced = false;
        db.insert_clip(&clip).unwrap();

        let clips = db.list_clips(None, None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert!(
            !clips[0].synced,
            "clip inserted with synced=false should read back as false"
        );
    }

    #[test]
    fn test_upsert_preserves_pin() {
        let db = test_db();

        // 1. Insert a clip initially
        let mut clip = make_clip("upsert-test", "original content", "remote:prod", "text");
        clip.synced = true;
        db.insert_clip(&clip).unwrap();

        // 2. Pin it with a note
        db.pin_clip("upsert-test", Some("my important note"))
            .unwrap();

        // 3. Verify it's pinned
        let pinned = db.list_pinned_clips().unwrap();
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].is_pinned, true);
        assert_eq!(pinned[0].pin_note, Some("my important note".to_string()));

        // 4. Upsert the same clip with different content (simulating relay re-delivery)
        let mut updated_clip = make_clip("upsert-test", "updated content", "remote:prod", "text");
        updated_clip.synced = true;
        updated_clip.is_pinned = false; // incoming clip doesn't know about pin state
        updated_clip.pin_note = None; // incoming clip has no pin note
        db.insert_clip(&updated_clip).unwrap();

        // 5. Verify the pin state is STILL present (not overwritten)
        let clips = db.list_clips(None, None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].id, "upsert-test");
        assert_eq!(
            clips[0].content, "updated content",
            "mutable fields should be updated"
        );
        assert_eq!(
            clips[0].is_pinned, true,
            "is_pinned should be preserved from local state"
        );
        assert_eq!(
            clips[0].pin_note,
            Some("my important note".to_string()),
            "pin_note should be preserved from local state"
        );

        // 6. Verify pinned list still shows it
        let pinned = db.list_pinned_clips().unwrap();
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].is_pinned, true);
    }

    #[test]
    fn test_upsert_preserves_synced() {
        let db = test_db();

        // 1. Insert a clip with synced=false (offline local push)
        let mut clip = make_clip("synced-upsert", "local content", "local", "text");
        clip.synced = false;
        db.insert_clip(&clip).unwrap();

        // 2. Verify it's unsynced
        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 1);
        assert_eq!(unsynced[0].synced, false);

        // 3. Upsert with new content but incoming synced=true (relay doesn't set our synced flag)
        let mut relay_clip =
            make_clip("synced-upsert", "updated from relay", "remote:prod", "text");
        relay_clip.synced = true; // relay clip always has synced=true
        db.insert_clip(&relay_clip).unwrap();

        // 4. Verify synced flag is STILL false (preserved)
        let clips = db.list_clips(None, None, 50).unwrap();
        assert_eq!(clips.len(), 1);
        assert_eq!(
            clips[0].synced, false,
            "synced should be preserved from local state"
        );
        assert_eq!(
            clips[0].content, "updated from relay",
            "mutable fields should be updated"
        );

        // 5. Verify unsynced list still shows it
        let unsynced = db.list_unsynced_clips().unwrap();
        assert_eq!(unsynced.len(), 1);
    }
}
