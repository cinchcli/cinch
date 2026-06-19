//! Full-text and structured clip search: query parsing, FTS sanitisation,
//! and the ranked `query_clips` / `search_clips` entry points.

use super::clips::{stored_clip_from_row, CLIP_COLUMNS, CLIP_COLUMNS_OMIT_IMAGE_CONTENT};
use super::models::StoredClip;
use super::{Store, StoreError};
use rusqlite::params;
use rusqlite::OptionalExtension;

#[derive(Debug, Default)]
pub struct ParsedQuery {
    pub from: Option<String>,
    /// Source-app bundle id from an `app:<bundle_id>` filter. Bundle ids never
    /// contain whitespace, so they survive the whitespace tokenizer intact —
    /// the human display name ("Google Chrome") would not, which is why the
    /// filter keys on the bundle id (mirrors `from:` keying on the source key).
    pub app: Option<String>,
    pub content_type: Option<String>,
    pub pinned: Option<bool>,
    pub search_term: String,
}

pub fn parse_query_string(raw: &str) -> ParsedQuery {
    let mut pq = ParsedQuery::default();
    let mut terms = Vec::new();

    for part in raw.split_whitespace() {
        if let Some(val) = part.strip_prefix("from:") {
            pq.from = Some(val.to_string());
        } else if let Some(val) = part.strip_prefix("app:") {
            pq.app = Some(val.to_string());
        } else if let Some(val) = part.strip_prefix("type:") {
            pq.content_type = Some(val.to_string());
        } else if part == "is:pinned" {
            pq.pinned = Some(true);
        } else {
            terms.push(part);
        }
    }
    pq.search_term = terms.join(" ");
    pq
}

/// Ranked clip search. `raw_query` may embed `from:`/`type:`/`is:pinned`
/// filters (parsed by [`parse_query_string`]); `exclude_source`, when `Some`,
/// adds an exact-EXCLUDE `source != ?` predicate so the MCP `scope:"fleet"`
/// read can drop this machine's own clips. See [`super::clips::list_clips`]
/// for the note on `source != ?` being a residual filter, not an index seek.
pub fn query_clips(
    store: &Store,
    raw_query: &str,
    limit: i64,
    exclude_source: Option<&str>,
) -> Result<Vec<StoredClip>, StoreError> {
    query_clips_with_columns(store, raw_query, limit, exclude_source, CLIP_COLUMNS)
}

/// Like [`query_clips`], but image rows come back with `content == None` (see
/// [`super::clips::CLIP_COLUMNS_OMIT_IMAGE_CONTENT`]). The desktop list and
/// search panes use this. The `WHERE` clause is unchanged, so matching and
/// ranking are identical — only the returned `content` projection differs.
pub fn query_clips_without_image_content(
    store: &Store,
    raw_query: &str,
    limit: i64,
    exclude_source: Option<&str>,
) -> Result<Vec<StoredClip>, StoreError> {
    query_clips_with_columns(
        store,
        raw_query,
        limit,
        exclude_source,
        CLIP_COLUMNS_OMIT_IMAGE_CONTENT,
    )
}

fn query_clips_with_columns(
    store: &Store,
    raw_query: &str,
    limit: i64,
    exclude_source: Option<&str>,
    columns: &str,
) -> Result<Vec<StoredClip>, StoreError> {
    let pq = parse_query_string(raw_query);
    let fts_query = sanitize_fts_query(&pq.search_term);
    let like_query = format!("%{}%", pq.search_term);

    store.with_conn(|conn| {
        let mut sql = format!("SELECT {columns} FROM clips WHERE 1=1");
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if !pq.search_term.is_empty() {
            sql.push_str(" AND (rowid IN (SELECT rowid FROM clips_fts WHERE clips_fts MATCH ?");
            binds.push(Box::new(fts_query));
            sql.push_str(") OR source_app LIKE ? OR source_url LIKE ? OR label LIKE ?");
            binds.push(Box::new(like_query.clone()));
            binds.push(Box::new(like_query.clone()));
            binds.push(Box::new(like_query.clone()));
            // Substring fallback on the clip body. FTS5 only matches whole
            // tokens — even with the trailing `*` we add in `sanitize_fts_query`
            // it can only match token *prefixes*, never a substring sitting in
            // the middle of a token. A plain `LIKE '%term%'` closes that gap so
            // "Muse" finds "AIMuse" and "진무" finds "고진무". It is guarded by
            // the image exclusion so image bytes stay searchable only by
            // metadata/label (the schema-v7 invariant), even under `type:image`.
            sql.push_str(" OR (CAST(content AS TEXT) LIKE ? AND content_type NOT LIKE 'image%'))");
            binds.push(Box::new(like_query.clone()));
        }

        if let Some(from_val) = pq.from {
            // Resolve the 'from' value to a source key via nicknames/hostnames.
            // Use the connection we already hold — a nested `store.with_conn()`
            // here deadlocks, since the underlying `Mutex<Connection>` is not
            // re-entrant. Resolution stays best-effort: any error falls through
            // to an exact match on the raw `from_val` below.
            let resolved_source: Option<String> = conn
                .prepare(
                    "SELECT source_key FROM devices
                     WHERE source_key = ?1 OR hostname = ?1 OR nickname = ?1
                     LIMIT 1",
                )
                .and_then(|mut stmt| {
                    stmt.query_row(params![from_val], |r| r.get::<_, String>(0))
                        .optional()
                })
                .unwrap_or(None);

            if let Some(s) = resolved_source {
                sql.push_str(" AND source = ?");
                binds.push(Box::new(s));
            } else {
                // Fallback to exact match on source column if not found in devices table
                sql.push_str(" AND source = ?");
                binds.push(Box::new(from_val));
            }
        }

        // Source-app filter (`app:<bundle_id>`). Exact match on the stored
        // bundle id — the same key the icon endpoint (`cinch://app-icon/<id>`)
        // uses. An empty value (a stray `app:` sigil) is ignored so it never
        // blanks the list. Independent of the `from:` device include.
        if let Some(app) = pq.app.filter(|a| !a.is_empty()) {
            sql.push_str(" AND source_app_id = ?");
            binds.push(Box::new(app));
        }

        // Fleet-read exclude-self predicate (scope:"fleet"). Independent of the
        // `from:` include above; the MCP layer passes the unresolved
        // self_source_key directly, so no device-table resolution is needed.
        if let Some(s) = exclude_source {
            sql.push_str(" AND source != ?");
            binds.push(Box::new(s.to_string()));
        }

        if let Some(ct) = pq.content_type {
            if ct == "image" {
                sql.push_str(" AND content_type LIKE 'image%'");
            } else {
                sql.push_str(" AND content_type = ?");
                binds.push(Box::new(ct));
            }
        } else if pq.search_term.is_empty() {
            // Default list: no hidden image filter
        } else {
            // Searching: hide images unless explicit
            sql.push_str(" AND content_type NOT LIKE 'image%'");
        }

        if let Some(true) = pq.pinned {
            sql.push_str(" AND pinned = 1");
        }

        if pq.search_term.is_empty() {
            sql.push_str(" ORDER BY created_at DESC");
        } else {
            sql.push_str(
                " ORDER BY (
                    CASE
                        WHEN label LIKE ? THEN 0
                        WHEN source_app LIKE ? OR source_url LIKE ? THEN 1
                        ELSE 2
                    END
                ) ASC, created_at DESC",
            );
            binds.push(Box::new(like_query.clone()));
            binds.push(Box::new(like_query.clone()));
            binds.push(Box::new(like_query.clone()));
        }

        sql.push_str(" LIMIT ?");
        binds.push(Box::new(limit));

        let mut stmt = conn.prepare(&sql)?;
        let rows: Vec<StoredClip> = stmt
            .query_map(
                rusqlite::params_from_iter(binds.iter().map(|b| &**b as &dyn rusqlite::ToSql)),
                stored_clip_from_row,
            )?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

/// Convenience wrapper over [`query_clips`] that appends an optional
/// `type:<filter_type>` term. `exclude_source` is threaded straight through
/// to power the MCP `scope:"fleet"` read on `search_clipboard`.
pub fn search_clips(
    store: &Store,
    query: &str,
    limit: i64,
    filter_type: Option<&str>,
    exclude_source: Option<&str>,
) -> Result<Vec<StoredClip>, StoreError> {
    let mut full_query = query.to_string();
    if let Some(t) = filter_type {
        full_query.push_str(&format!(" type:{}", t));
    }
    query_clips(store, &full_query, limit, exclude_source)
}

/// Like [`search_clips`], but image rows come back with `content == None`
/// (see [`query_clips_without_image_content`]). Used by the desktop search
/// pane, which renders image bytes out-of-band via `cinch://media/`.
pub fn search_clips_without_image_content(
    store: &Store,
    query: &str,
    limit: i64,
    filter_type: Option<&str>,
    exclude_source: Option<&str>,
) -> Result<Vec<StoredClip>, StoreError> {
    let mut full_query = query.to_string();
    if let Some(t) = filter_type {
        full_query.push_str(&format!(" type:{}", t));
    }
    query_clips_without_image_content(store, &full_query, limit, exclude_source)
}

/// Make an arbitrary natural-language query safe for SQLite FTS5 `MATCH`.
/// FTS5 treats `-`, `:`, `"`, `*`, `(`, `)`, `^`, and bare-word operators
/// (`AND`/`OR`/`NEAR`) specially; raw AI/user input often produces syntax
/// errors. We split on whitespace and wrap each token as a quoted FTS5 string
/// (internal `"` doubled), then append `*` so each term is a **prefix** query —
/// a partial word like "2018" matches the longer indexed token "201813124"
/// instead of requiring an exact whole-token hit. Tokens are joined with
/// spaces (implicit AND). Whitespace-only input yields `""`, which callers
/// treat as "no FTS filter". (Mid-token substrings, e.g. "Muse" inside
/// "AIMuse", are out of reach for FTS prefixing — `query_clips` adds a
/// `content LIKE` fallback for those.)
pub fn sanitize_fts_query(raw: &str) -> String {
    raw.split_whitespace()
        .map(|tok| format!("\"{}\"*", tok.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::super::clips::insert_clip;
    use super::super::models::{StoredClip, SyncState};
    use super::*;

    fn text_clip(id: &str, content: &str) -> StoredClip {
        StoredClip {
            id: id.into(),
            source: "s".into(),
            content_type: "text".into(),
            content: Some(content.as_bytes().to_vec()),
            byte_size: content.len() as i64,
            created_at: 1,
            sync_state: SyncState::Synced,
            ..Default::default()
        }
    }

    #[test]
    fn parse_query_string_extracts_app_filter() {
        // `app:<bundle_id>` is lifted out as a structured filter, not a search
        // term, and coexists with the existing from:/type: filters.
        let pq = parse_query_string("app:com.apple.Safari hello world");
        assert_eq!(pq.app.as_deref(), Some("com.apple.Safari"));
        assert_eq!(pq.search_term, "hello world");

        let pq2 = parse_query_string("from:laptop app:com.microsoft.VSCode type:url");
        assert_eq!(pq2.app.as_deref(), Some("com.microsoft.VSCode"));
        assert_eq!(pq2.from.as_deref(), Some("laptop"));
        assert_eq!(pq2.content_type.as_deref(), Some("url"));
        assert_eq!(pq2.search_term, "");
    }

    fn app_clip(id: &str, bundle_id: Option<&str>, app_name: Option<&str>) -> StoredClip {
        StoredClip {
            id: id.into(),
            source: "s".into(),
            source_app_id: bundle_id.map(Into::into),
            source_app: app_name.map(Into::into),
            content_type: "text".into(),
            content: Some(b"body".to_vec()),
            byte_size: 4,
            created_at: 1,
            sync_state: SyncState::Synced,
            ..Default::default()
        }
    }

    #[test]
    fn query_clips_filters_by_app() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        insert_clip(
            &store,
            &app_clip("c_safari", Some("com.apple.Safari"), Some("Safari")),
        )
        .unwrap();
        insert_clip(
            &store,
            &app_clip("c_code", Some("com.microsoft.VSCode"), Some("Code")),
        )
        .unwrap();

        // app:<bundle_id> returns only clips captured from that app.
        let hits = query_clips(&store, "app:com.apple.Safari", 10, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, "c_safari");

        // Combines with a free-text term: app filter AND content match.
        let combined = query_clips(&store, "app:com.microsoft.VSCode body", 10, None).unwrap();
        assert_eq!(combined.len(), 1);
        assert_eq!(combined[0].id, "c_code");

        // An empty `app:` value is a no-op, never blanking the list.
        let all = query_clips(&store, "app:", 10, None).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn sanitize_fts_query_uses_prefix_tokens() {
        // Each whitespace-separated term becomes a quoted FTS5 *prefix* token so
        // a partial word matches a longer indexed token ("2018" → "201813124").
        assert_eq!(sanitize_fts_query("2018"), "\"2018\"*");
        assert_eq!(sanitize_fts_query("hello world"), "\"hello\"* \"world\"*");
        assert_eq!(sanitize_fts_query("   "), "");
    }

    #[test]
    fn search_matches_prefix_of_a_longer_token() {
        // The reported bug: "2018" must find "201813124_고진무_AIMuse_앱기획".
        // The underscore splits the content into tokens [201813124, 고진무,
        // aimuse, 앱기획]; "2018" is a *prefix* of the first token, not a whole
        // token, so the old exact-match query returned nothing.
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        insert_clip(&store, &text_clip("c1", "201813124_고진무_AIMuse_앱기획")).unwrap();

        let hits = query_clips(&store, "2018", 10, None).unwrap();
        assert_eq!(hits.len(), 1, "prefix '2018' must match '201813124_…'");
        assert_eq!(hits[0].id, "c1");
    }

    #[test]
    fn search_matches_substring_in_middle_of_a_token() {
        // Substrings that are not token prefixes — FTS5 (even with a trailing
        // `*`) can't reach these; the content LIKE fallback does.
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        insert_clip(&store, &text_clip("c1", "201813124_고진무_AIMuse_앱기획")).unwrap();

        // "Muse" sits inside the token "AIMuse" (case-insensitive).
        assert_eq!(query_clips(&store, "Muse", 10, None).unwrap().len(), 1);
        // "진무" (2 chars) sits inside the Korean token "고진무".
        assert_eq!(query_clips(&store, "진무", 10, None).unwrap().len(), 1);
        // "13124" sits inside the numeric token "201813124".
        assert_eq!(query_clips(&store, "13124", 10, None).unwrap().len(), 1);
    }

    #[test]
    fn substring_fallback_still_excludes_image_content() {
        // Schema-v7 invariant: image bytes are searchable only by metadata/label,
        // never by content — the substring fallback must honor that even under an
        // explicit `type:image` filter.
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut img = text_clip("img", "201813124 needle base64");
        img.content_type = "image/png".into();
        insert_clip(&store, &img).unwrap();

        assert!(
            query_clips(&store, "2018", 10, None).unwrap().is_empty(),
            "image content must not be reachable via the substring fallback"
        );
        assert!(
            search_clips(&store, "needle", 10, Some("image"), None)
                .unwrap()
                .is_empty(),
            "type:image search must not match image *content*, only metadata"
        );
    }

    #[test]
    fn query_clips_without_image_content_nulls_images_keeps_text() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        insert_clip(&store, &text_clip("t", "hello world")).unwrap();
        let mut img = text_clip("i", "rawimagebytes");
        img.content_type = "image/png".into();
        insert_clip(&store, &img).unwrap();

        // Empty query → default newest-first list, via the omitting projection.
        let rows = query_clips_without_image_content(&store, "", 10, None).unwrap();
        let t = rows.iter().find(|c| c.id == "t").unwrap();
        let i = rows.iter().find(|c| c.id == "i").unwrap();
        assert_eq!(
            t.content.as_deref(),
            Some(&b"hello world"[..]),
            "text content must survive the image-omitting projection"
        );
        assert!(
            i.content.is_none(),
            "image content must be NULL in the image-omitting projection"
        );
    }

    #[test]
    fn query_clips_still_returns_image_content() {
        // Regression guard: non-desktop callers (CLI/MCP) of the search path
        // must keep receiving image bytes.
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut img = text_clip("i", "rawimagebytes");
        img.content_type = "image/png".into();
        insert_clip(&store, &img).unwrap();

        let rows = query_clips(&store, "", 10, None).unwrap();
        assert_eq!(rows[0].content.as_deref(), Some(&b"rawimagebytes"[..]));
    }

    #[test]
    fn search_clips_without_image_content_omits_image_under_type_filter() {
        // type:image surfaces image rows by metadata; through the omitting
        // projection their content must be NULL (the desktop renders image
        // bytes via cinch://media/, never from a list result).
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let mut img = text_clip("i", "rawimagebytes");
        img.content_type = "image/png".into();
        img.label = Some("screenshot".into());
        insert_clip(&store, &img).unwrap();

        let rows =
            search_clips_without_image_content(&store, "screenshot", 10, Some("image"), None)
                .unwrap();
        assert_eq!(rows.len(), 1, "label search must surface the image row");
        assert!(rows[0].content.is_none(), "image content must be omitted");
    }
}
