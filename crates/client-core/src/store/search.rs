//! Full-text and structured clip search: query parsing, FTS sanitisation,
//! and the ranked `query_clips` / `search_clips` entry points.

use super::clips::{stored_clip_from_row, CLIP_COLUMNS};
use super::models::StoredClip;
use super::{Store, StoreError};
use rusqlite::params;
use rusqlite::OptionalExtension;

#[derive(Debug, Default)]
pub struct ParsedQuery {
    pub from: Option<String>,
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

pub fn query_clips(
    store: &Store,
    raw_query: &str,
    limit: i64,
) -> Result<Vec<StoredClip>, StoreError> {
    let pq = parse_query_string(raw_query);
    let fts_query = sanitize_fts_query(&pq.search_term);
    let like_query = format!("%{}%", pq.search_term);

    store.with_conn(|conn| {
        let mut sql = format!("SELECT {CLIP_COLUMNS} FROM clips WHERE 1=1");
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        if !pq.search_term.is_empty() {
            sql.push_str(" AND (rowid IN (SELECT rowid FROM clips_fts WHERE clips_fts MATCH ?");
            binds.push(Box::new(fts_query));
            sql.push_str(") OR source_app LIKE ? OR source_url LIKE ? OR label LIKE ?)");
            binds.push(Box::new(like_query.clone()));
            binds.push(Box::new(like_query.clone()));
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

pub fn search_clips(
    store: &Store,
    query: &str,
    limit: i64,
    filter_type: Option<&str>,
) -> Result<Vec<StoredClip>, StoreError> {
    let mut full_query = query.to_string();
    if let Some(t) = filter_type {
        full_query.push_str(&format!(" type:{}", t));
    }
    query_clips(store, &full_query, limit)
}

/// Make an arbitrary natural-language query safe for SQLite FTS5 `MATCH`.
/// FTS5 treats `-`, `:`, `"`, `*`, `(`, `)`, `^`, and bare-word operators
/// (`AND`/`OR`/`NEAR`) specially; raw AI/user input often produces syntax
/// errors. We split on whitespace and wrap each token as a quoted FTS5 string
/// (internal `"` doubled), joined with spaces (implicit AND). Whitespace-only
/// input yields `""`, which callers treat as "no FTS filter".
pub fn sanitize_fts_query(raw: &str) -> String {
    raw.split_whitespace()
        .map(|tok| format!("\"{}\"", tok.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}
