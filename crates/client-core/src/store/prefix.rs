use super::models::{MatchInfo, ResolveError};
use super::Store;
use rusqlite::params;

pub const MIN_PREFIX: usize = 4;
const MAX_CANDIDATES: i64 = 6;

pub fn resolve_clip_id(store: &Store, prefix: &str) -> Result<String, ResolveError> {
    if prefix.len() < MIN_PREFIX {
        return Err(ResolveError::TooShort);
    }
    let matches = query_matches(
        store,
        "SELECT id, source, content_type, created_at, COALESCE(SUBSTR(CAST(content AS TEXT), 1, 40), '') AS preview, label
         FROM clips WHERE id LIKE ?1 ORDER BY created_at DESC LIMIT ?2",
        prefix,
    )?;
    resolve_unique(matches)
}

pub fn resolve_device_id(store: &Store, prefix: &str) -> Result<String, ResolveError> {
    if prefix.len() < MIN_PREFIX {
        return Err(ResolveError::TooShort);
    }
    let matches = query_matches(
        store,
        "SELECT id, hostname AS source, '' AS content_type, COALESCE(last_push_at, 0), COALESCE(nickname, hostname), NULL AS label
         FROM devices WHERE id LIKE ?1 LIMIT ?2",
        prefix,
    )?;
    resolve_unique(matches)
}

/// Run a prefix lookup and map each row into a [`MatchInfo`].
///
/// `sql` must select the six columns in `MatchInfo` order — `id`, `source`,
/// `content_type`, `created_at`, `preview`, `label` — and bind the prefix
/// pattern as `?1` and the candidate cap as `?2`. Callers own the
/// [`MIN_PREFIX`] guard before calling.
fn query_matches(store: &Store, sql: &str, prefix: &str) -> Result<Vec<MatchInfo>, ResolveError> {
    let pattern = format!("{prefix}%");
    let matches: Vec<MatchInfo> = store.with_conn(|conn| {
        let mut stmt = conn.prepare(sql)?;
        let rows: Vec<MatchInfo> = stmt
            .query_map(params![pattern, MAX_CANDIDATES], |r| {
                Ok(MatchInfo {
                    id: r.get(0)?,
                    source: r.get(1)?,
                    content_type: r.get(2)?,
                    created_at: r.get(3)?,
                    preview: r.get(4)?,
                    label: r.get(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })?;
    Ok(matches)
}

/// Collapse a candidate list to a single id: none -> `NotFound`, exactly one ->
/// its id, more than one -> `Ambiguous` carrying the candidates for the caller
/// to render.
fn resolve_unique(mut matches: Vec<MatchInfo>) -> Result<String, ResolveError> {
    match matches.len() {
        0 => Err(ResolveError::NotFound),
        1 => Ok(matches.remove(0).id),
        _ => Err(ResolveError::Ambiguous {
            candidates: matches,
        }),
    }
}
