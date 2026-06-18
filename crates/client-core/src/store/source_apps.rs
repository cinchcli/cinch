//! The clip-derived "apps you've copied from" list that backs the search
//! bar's `app:` filter picker (mirrors [`super::devices::list_sources`]).

use super::models::SourceAppRow;
use super::{Store, StoreError};

/// Distinct source apps across all clips, most-used first.
///
/// Grouped by bundle id; clips with no captured bundle id (null or empty
/// `source_app_id`) are excluded — they have no stable, whitespace-free key to
/// filter on (they remain reachable via free-text search). `app_name` is the
/// display name via `MAX(source_app)`, falling back (in SQL, via `COALESCE`) to
/// the bundle id when no row for that bundle captured a name — so the picker is
/// never blank. Doing the fallback in SQL keeps the `ORDER BY` keyed on the
/// *displayed* name: a count-tied app with a NULL name would otherwise sort
/// ahead of named apps (SQL puts NULL first) yet display as its bundle id.
/// Ordered by clip count descending, then displayed name ascending.
pub fn list_source_apps(store: &Store) -> Result<Vec<SourceAppRow>, StoreError> {
    store.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT source_app_id,
                    COALESCE(MAX(source_app), source_app_id) AS app_name,
                    COUNT(*) AS c
             FROM clips
             WHERE source_app_id IS NOT NULL AND source_app_id <> ''
             GROUP BY source_app_id
             ORDER BY c DESC, app_name ASC",
        )?;
        let rows: Vec<SourceAppRow> = stmt
            .query_map([], |r| {
                Ok(SourceAppRow {
                    app_id: r.get(0)?,
                    app_name: r.get(1)?,
                    count: r.get(2)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

#[cfg(test)]
mod tests {
    use super::super::clips::insert_clip;
    use super::super::models::{StoredClip, SyncState};
    use super::*;

    fn clip(id: &str, bundle_id: Option<&str>, app_name: Option<&str>) -> StoredClip {
        StoredClip {
            id: id.into(),
            source: "s".into(),
            source_app_id: bundle_id.map(Into::into),
            source_app: app_name.map(Into::into),
            content_type: "text".into(),
            content: Some(b"x".to_vec()),
            byte_size: 1,
            created_at: 1,
            sync_state: SyncState::Synced,
            ..Default::default()
        }
    }

    #[test]
    fn list_source_apps_groups_counts_and_excludes_null_bundle() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        // Two Safari clips (one missing its display name → name via MAX).
        insert_clip(
            &store,
            &clip("s1", Some("com.apple.Safari"), Some("Safari")),
        )
        .unwrap();
        insert_clip(&store, &clip("s2", Some("com.apple.Safari"), None)).unwrap();
        // One VSCode clip.
        insert_clip(
            &store,
            &clip("v1", Some("com.microsoft.VSCode"), Some("Code")),
        )
        .unwrap();
        // Null and empty bundle ids are excluded from the picker.
        insert_clip(&store, &clip("n1", None, Some("Mystery"))).unwrap();
        insert_clip(&store, &clip("e1", Some(""), Some("Blank"))).unwrap();

        let apps = list_source_apps(&store).unwrap();

        assert_eq!(apps.len(), 2, "null/empty bundle ids must be excluded");
        // Ordered by count desc: Safari (2) before VSCode (1).
        assert_eq!(apps[0].app_id, "com.apple.Safari");
        assert_eq!(
            apps[0].app_name, "Safari",
            "name resolved via MAX over NULL"
        );
        assert_eq!(apps[0].count, 2);
        assert_eq!(apps[1].app_id, "com.microsoft.VSCode");
        assert_eq!(apps[1].app_name, "Code");
        assert_eq!(apps[1].count, 1);
    }

    #[test]
    fn list_source_apps_orders_by_displayed_name_when_name_is_null() {
        // An app whose clips never captured a display name (source_app NULL on
        // every row) displays as its bundle id (the fallback). The list order
        // must match that *displayed* name, not the raw NULL — otherwise a
        // count-tied NULL-name app would sort ahead of a named one. "Beta"
        // (named) must precede "com.zzz.app" (bundle-id fallback) alphabetically.
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        insert_clip(&store, &clip("z1", Some("com.zzz.app"), None)).unwrap();
        insert_clip(&store, &clip("z2", Some("com.zzz.app"), None)).unwrap();
        insert_clip(&store, &clip("b1", Some("com.beta"), Some("Beta"))).unwrap();
        insert_clip(&store, &clip("b2", Some("com.beta"), Some("Beta"))).unwrap();

        let apps = list_source_apps(&store).unwrap();

        assert_eq!(apps.len(), 2);
        assert_eq!(
            apps[0].app_name, "Beta",
            "named app must precede the bundle-id-fallback app on a count tie"
        );
        assert_eq!(apps[1].app_id, "com.zzz.app");
        assert_eq!(
            apps[1].app_name, "com.zzz.app",
            "NULL display name surfaces as the bundle id"
        );
    }
}
