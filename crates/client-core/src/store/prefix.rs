use super::models::{MatchInfo, ResolveError};
use super::Store;
use rusqlite::params;

pub const MIN_PREFIX: usize = 4;
const MAX_CANDIDATES: i64 = 6;

pub fn resolve_clip_id(store: &Store, prefix: &str) -> Result<String, ResolveError> {
    if prefix.len() < MIN_PREFIX {
        return Err(ResolveError::TooShort);
    }
    let pattern = format!("{prefix}%");
    let matches: Vec<MatchInfo> = store.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, source, content_type, created_at, COALESCE(SUBSTR(CAST(content AS TEXT), 1, 40), '') AS preview
             FROM clips WHERE id LIKE ?1 ORDER BY created_at DESC LIMIT ?2"
        )?;
        let rows: Vec<MatchInfo> = stmt
            .query_map(params![pattern, MAX_CANDIDATES], |r| {
                Ok(MatchInfo {
                    id: r.get(0)?,
                    source: r.get(1)?,
                    content_type: r.get(2)?,
                    created_at: r.get(3)?,
                    preview: r.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })?;
    match matches.as_slice() {
        [] => Err(ResolveError::NotFound),
        [one] => Ok(one.id.clone()),
        _many => Err(ResolveError::Ambiguous {
            candidates: matches,
        }),
    }
}

pub fn resolve_device_id(store: &Store, prefix: &str) -> Result<String, ResolveError> {
    if prefix.len() < MIN_PREFIX {
        return Err(ResolveError::TooShort);
    }
    let pattern = format!("{prefix}%");
    let mut matches: Vec<MatchInfo> = store.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, hostname AS source, '' AS content_type, COALESCE(last_push_at, 0), COALESCE(nickname, hostname)
             FROM devices WHERE id LIKE ?1 LIMIT ?2"
        )?;
        let rows: Vec<MatchInfo> = stmt
            .query_map(params![pattern, MAX_CANDIDATES], |r| {
                Ok(MatchInfo {
                    id: r.get(0)?,
                    source: r.get(1)?,
                    content_type: r.get(2)?,
                    created_at: r.get(3)?,
                    preview: r.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })?;
    match matches.len() {
        0 => Err(ResolveError::NotFound),
        1 => Ok(matches.remove(0).id),
        _ => Err(ResolveError::Ambiguous {
            candidates: matches,
        }),
    }
}
