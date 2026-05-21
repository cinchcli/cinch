//! `cinch list` — recent clips with previews and metadata.

use serde::Serialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use client_core::auth::load_config;
use client_core::config::Config;
use client_core::credstore;
use client_core::crypto;
use client_core::http::RestClient;
use client_core::protocol::Clip;
use client_core::store::models::StoredClip;

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR, RELAY_ERROR};

pub const LIST_INLINE_LIMIT_BYTES: usize = 10 * 1024;
pub const PREVIEW_MAX_CHARS: usize = 200;

#[derive(Serialize, Debug)]
pub struct ListRecord {
    pub id: String,
    pub source: String,
    pub source_name: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub created_at: String,
    pub preview: String,
    pub content: Option<String>,
    pub image_metadata: Option<ImageMetadata>,
}

#[derive(Serialize, Debug)]
pub struct ImageMetadata {
    pub mime: String,
}

fn render_record(clip: &Clip, source_name: &str) -> ListRecord {
    let is_image = clip.content_type.starts_with("image");
    let preview = if is_image {
        let kb = clip.byte_size as f64 / 1024.0;
        let size_str = if kb >= 1024.0 {
            format!("{:.1} MB", kb / 1024.0)
        } else {
            format!("{:.0} KB", kb)
        };
        format!("[image · {} · {}]", size_str, clip.content_type)
    } else {
        let first_line = clip.content.lines().next().unwrap_or("");
        truncate(first_line, PREVIEW_MAX_CHARS)
    };
    let content = if !is_image && clip.content.len() <= LIST_INLINE_LIMIT_BYTES {
        Some(clip.content.clone())
    } else {
        None
    };
    let image_metadata = if is_image {
        Some(ImageMetadata {
            mime: clip.content_type.clone(),
        })
    } else {
        None
    };
    ListRecord {
        id: clip.clip_id.clone(),
        source: clip.source.clone(),
        source_name: source_name.into(),
        content_type: clip.content_type.clone(),
        size_bytes: clip.byte_size,
        created_at: clip.created_at.clone(),
        preview,
        content,
        image_metadata,
    }
}

fn humanize_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{}s", s)
    } else if s < 3600 {
        format!("{}m", s / 60)
    } else if s < 86_400 {
        format!("{}h", s / 3600)
    } else {
        format!("{}d", s / 86_400)
    }
}

/// Strip the canonical `remote:` prefix from a source string so the device
/// name reads cleanly in the table column.
fn normalize_source(s: &str) -> &str {
    s.strip_prefix("remote:").unwrap_or(s)
}

/// Abbreviate `content_type` for the 4-character type column. Mirrors
/// `lua/cinch/format.lua::compact_type` in cinch.vim so picker output and CLI
/// output stay consistent.
///
/// `text/*` and `image/*` are accepted in addition to the canonical 4 strings
/// — pre-0510e1f desktop builds emitted MIME-style values and the relay
/// never rewrites the open `content_type` string, so legacy clips show up
/// here verbatim.
fn compact_type(t: &str) -> &'static str {
    if t.starts_with("image") {
        return "img";
    }
    if t.starts_with("text") {
        return "text";
    }
    match t {
        "code" => "code",
        "url" => "url",
        _ => "?",
    }
}

/// Source column width = max(`min`, longest normalized source name). Lets the
/// table stay aligned regardless of which devices appear in the result set.
fn source_width(recs: &[ListRecord], min: usize) -> usize {
    let mut w = min;
    for r in recs {
        let n = normalize_source(&r.source_name).chars().count();
        if n > w {
            w = n;
        }
    }
    w
}

// Howard Hinnant's days_from_civil — converts a (year, month, day) Gregorian
// date into days since 1970-01-01. Public domain.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u32;
    let m_u = m as i64;
    let d_u = d as i64;
    let doy = (153 * (if m_u > 2 { m_u - 3 } else { m_u + 9 }) + 2) / 5 + d_u - 1;
    let doe = (yoe as i64) * 365 + (yoe as i64) / 4 - (yoe as i64) / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn parse_rfc3339(s: &str) -> Option<SystemTime> {
    // Accepts YYYY-MM-DDTHH:MM:SS(.fff)?Z — the format emitted by protocol.FormatRFC3339.
    // Strict enough for relay-produced timestamps; returns None on malformed input.
    let trimmed = s.trim_end_matches('Z');
    let (date, time) = trimmed.split_once('T')?;

    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;

    // Drop fractional seconds if present.
    let time_main = time.split('.').next()?;
    let mut t = time_main.split(':');
    let hour: u32 = t.next()?.parse().ok()?;
    let min: u32 = t.next()?.parse().ok()?;
    let sec: u32 = t.next()?.parse().ok()?;

    // Compute days since UNIX_EPOCH using Howard Hinnant's algorithm.
    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + (hour as i64) * 3600 + (min as i64) * 60 + sec as i64;
    if secs < 0 {
        return None;
    }
    Some(UNIX_EPOCH + Duration::from_secs(secs as u64))
}

fn render_table(recs: &[ListRecord], now: SystemTime) -> String {
    // Layout: source(sw)  type(4)  age(3)  preview
    // sw = max(8, longest normalized source name) so the table aligns to the
    // widest device shown. Mirrors the picker format in cinch.vim.
    let sw = source_width(recs, 8);
    let mut out = String::new();
    for r in recs {
        let age = match parse_rfc3339(&r.created_at) {
            Some(created) => now
                .duration_since(created)
                .map(humanize_duration)
                .unwrap_or_else(|_| "now".to_string()),
            None => "?".to_string(),
        };
        let src = normalize_source(&r.source_name);
        let typ = compact_type(&r.content_type);
        out.push_str(&format!(
            "{:<sw$}  {:<4}  {:<3}  {}\n",
            src,
            typ,
            age,
            r.preview,
            sw = sw,
        ));
    }
    out
}

fn truncate(s: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(max_chars);
    for (i, ch) in s.chars().enumerate() {
        if i >= max_chars {
            break;
        }
        out.push(ch);
    }
    out
}

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Max number of clips to return. Hard cap is 200.
    #[arg(long, default_value_t = 50)]
    pub limit: u32,

    /// Filter by source device (nickname or hostname).
    #[arg(long)]
    pub from: Option<String>,

    /// Drop image clips.
    #[arg(long = "text-only")]
    pub text_only: bool,

    /// Drop clips authored by this device.
    #[arg(long = "exclude-self")]
    pub exclude_self: bool,

    /// Force JSON output (default when stdout is not a TTY).
    #[arg(long)]
    pub json: bool,

    /// Bypass the local store and fetch directly from the relay.
    #[arg(long)]
    pub remote: bool,

    /// Show only pinned clips.
    #[arg(long)]
    pub pinned: bool,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    // --remote: fall back to the legacy relay-fetch path unchanged.
    if args.remote {
        if args.pinned {
            eprintln!(
                "note: --pinned is ignored with --remote (the relay has no pinned filter); \
                 falling back to unfiltered remote listing."
            );
        }
        return run_remote(args).await;
    }

    // Default: read from the local store with an opportunistic backfill.
    let ctx = crate::runtime::open_ctx()
        .map_err(|e| ExitError::new(AUTH_FAILURE, e, "Run: cinch auth login"))?;

    crate::runtime::opportunistic_backfill(&ctx).await;

    // --text-only and --exclude-self are relay-only filter hints; emit a
    // soft warning and fall through to remote when they are used without
    // --remote so the user gets the expected output.
    if args.text_only || args.exclude_self {
        eprintln!(
            "note: --text-only / --exclude-self are only supported with --remote; \
             switching to relay fetch."
        );
        return run_remote(args).await;
    }

    // Resolve --from to a source string stored in the local DB.
    // The local store uses the raw `source` column (e.g. "remote:hostname"),
    // so we first try to match against stored devices, then fall back to the
    // literal string supplied by the user.
    let local_from: Option<String> = if let Some(ref name) = args.from {
        // Attempt to look up via stored devices; fall back to literal value.
        let stored_devices =
            client_core::store::queries::list_devices(&ctx.store).unwrap_or_default();
        let lower = name.to_lowercase();
        stored_devices
            .iter()
            .find(|d| {
                let nick_match = d
                    .nickname
                    .as_deref()
                    .map(|n| n.to_lowercase() == lower)
                    .unwrap_or(false);
                let host_match = d.hostname.to_lowercase() == lower;
                nick_match || host_match
            })
            .and_then(|d| d.source_key.clone())
            .or_else(|| Some(name.clone()))
    } else {
        None
    };

    let rows = client_core::store::queries::list_clips(
        &ctx.store,
        local_from.as_deref(),
        Some(args.limit as i64),
        None,        // since_ms: not yet exposed as a CLI flag
        args.pinned, // pinned_only: propagated from --pinned flag
        args.limit as i64,
    )
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Local store read failed: {}", e), ""))?;

    let cfg = client_core::auth::load_config()
        .map_err(|e| ExitError::new(AUTH_FAILURE, format!("Could not load config: {}", e), ""))?;

    let mut clips: Vec<Clip> = rows.into_iter().map(into_wire_clip).collect();

    // Decrypt encrypted clips using the stored key (same logic as the relay path).
    for clip in clips.iter_mut() {
        if clip.encrypted {
            decrypt_clip_in_place(&cfg, clip)?;
        }
    }

    // For the local path, source_name is just the source string itself
    // (we don't have device nicknames easily at hand here).
    let records: Vec<ListRecord> = clips.iter().map(|c| render_record(c, &c.source)).collect();

    let json_mode = args.json || !std::io::IsTerminal::is_terminal(&std::io::stdout());
    if json_mode {
        let s = serde_json::to_string(&records).map_err(|e| {
            ExitError::new(
                GENERIC_ERROR,
                format!("Serialize list output failed: {}", e),
                "",
            )
        })?;
        println!("{}", s);
    } else {
        let table = render_table(&records, SystemTime::now());
        print!("{}", table);
    }
    Ok(())
}

/// Legacy relay-fetch path — used when `--remote` is passed.
async fn run_remote(args: Args) -> Result<(), ExitError> {
    let cfg = load_config().map_err(|e| {
        ExitError::new(
            AUTH_FAILURE,
            format!("Could not load config: {}", e),
            "Run: cinch auth login",
        )
    })?;
    if cfg.token.is_empty() {
        return Err(ExitError::new(
            AUTH_FAILURE,
            "No auth token configured.",
            "Run: cinch auth login",
        ));
    }
    let client = RestClient::new(
        cfg.relay_url.clone(),
        cfg.token.clone(),
        crate::client_info::for_cli(),
    )
    .map_err(|e| ExitError::new(GENERIC_ERROR, format!("Could not init client: {}", e), ""))?;

    let devices = client.list_devices().await.unwrap_or_default();
    let resolved_from = args
        .from
        .as_ref()
        .map(|name| resolve_source(&devices, name));
    let self_key = client_core::machine::self_source_key();
    let filter = build_filter(&args, &self_key, resolved_from);

    let mut clips = client
        .list_clips(filter)
        .await
        .map_err(|e| ExitError::new(RELAY_ERROR, format!("List clips failed: {}", e), ""))?;

    // Decrypt each clip if it carries encrypted content (mirrors pull.rs decrypt_clip flow).
    for clip in clips.iter_mut() {
        if clip.encrypted {
            decrypt_clip_in_place(&cfg, clip)?;
        }
    }

    // Resolve device source_name display once (reuse already-fetched devices vec).
    let resolve_name = |source: &str| -> String {
        devices
            .iter()
            .find(|d| d.source_key == source)
            .map(|d| {
                if d.nickname.is_empty() {
                    d.hostname.clone()
                } else {
                    d.nickname.clone()
                }
            })
            .unwrap_or_else(|| source.to_string())
    };

    let records: Vec<ListRecord> = clips
        .iter()
        .map(|c| render_record(c, &resolve_name(&c.source)))
        .collect();

    // JSON when requested or when not on a TTY.
    let json_mode = args.json || !std::io::IsTerminal::is_terminal(&std::io::stdout());
    if json_mode {
        let s = serde_json::to_string(&records).map_err(|e| {
            ExitError::new(
                GENERIC_ERROR,
                format!("Serialize list output failed: {}", e),
                "",
            )
        })?;
        println!("{}", s);
    } else {
        let table = render_table(&records, SystemTime::now());
        print!("{}", table);
    }
    Ok(())
}

/// Maps a `StoredClip` row to the wire `Clip` type used by the rest of this module.
fn into_wire_clip(c: StoredClip) -> Clip {
    let created_at_rfc = format_unix_ms_as_rfc3339(c.created_at);
    let content_str = match c.content {
        Some(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        None => String::new(),
    };
    Clip {
        clip_id: c.id,
        user_id: String::new(), // not stored locally
        content: content_str,
        content_type: c.content_type,
        source: c.source,
        label: String::new(),
        byte_size: c.byte_size,
        media_path: c.media_path,
        created_at: created_at_rfc,
        encrypted: false, // local store only holds plaintext
        is_pinned: c.pinned,
        pin_note: None,
    }
}

/// Formats a unix-millisecond timestamp as an RFC-3339 / ISO-8601 UTC string.
/// Produces `"1970-01-01T00:00:00Z"` for zero or out-of-range inputs.
pub(crate) fn format_unix_ms_as_rfc3339(ms: i64) -> String {
    if ms < 0 {
        return "1970-01-01T00:00:00Z".to_string();
    }
    let total_secs = ms / 1_000;
    // Days since epoch → Gregorian date (inverse of days_from_civil).
    let z = total_secs / 86_400;
    let rem_secs = total_secs % 86_400;
    let hour = rem_secs / 3_600;
    let min = (rem_secs % 3_600) / 60;
    let sec = rem_secs % 60;

    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let z = z + 719_468;
    let era: i64 = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m, d, hour, min, sec
    )
}

/// Resolves a device nickname/hostname to its `source_key`.
/// Takes a pre-fetched devices slice (caller already called `list_devices()`).
/// Falls back to `remote:<name>` when no device matches.
fn resolve_source(devices: &[client_core::protocol::DeviceInfo], from: &str) -> String {
    let lower = from.to_lowercase();
    for d in devices {
        let nick_match = !d.nickname.is_empty() && d.nickname.to_lowercase() == lower;
        let host_match = d.hostname.to_lowercase() == lower;
        if nick_match || host_match {
            return d.source_key.clone();
        }
    }
    format!("remote:{}", from)
}

/// Assembles a `ListClipsFilter` from parsed CLI args plus pre-resolved values.
/// Note: `args.pinned` is intentionally not forwarded — the relay REST endpoint has no pinned filter.
fn build_filter(
    args: &Args,
    self_key: &str,
    resolved_from: Option<String>,
) -> client_core::http::ListClipsFilter {
    client_core::http::ListClipsFilter {
        limit: args.limit,
        source: resolved_from,
        exclude_source: if args.exclude_self {
            Some(self_key.to_string())
        } else {
            None
        },
        exclude_image: args.text_only,
        exclude_text: false,
        clip_ids: vec![],
    }
}

/// Decrypts an encrypted clip in place. Mirrors the logic in `pull.rs::decrypt_clip`.
/// For text clips, content is replaced with valid UTF-8 plaintext.
/// For image clips, raw bytes are stored via `from_utf8_lossy` (callers using
/// the image branch recover bytes from `clip.content.into_bytes()`).
fn decrypt_clip_in_place(cfg: &Config, clip: &mut Clip) -> Result<(), ExitError> {
    let key = credstore::read_encryption_key(&cfg.user_id).ok_or_else(|| {
        ExitError::new(
            GENERIC_ERROR,
            "Clip is encrypted but no encryption key found.",
            "Run: cinch auth login to generate a key, or open desktop app for key exchange.",
        )
    })?;
    let plaintext = crypto::decrypt(&key, &clip.content).map_err(|e| {
        ExitError::new(
            GENERIC_ERROR,
            format!("Decryption failed: {}", e),
            "Encryption key may be wrong. Try: cinch auth login",
        )
    })?;
    let is_image = clip.content_type.starts_with("image");
    if is_image {
        clip.content = String::from_utf8_lossy(&plaintext).into_owned();
    } else {
        clip.content = String::from_utf8(plaintext).map_err(|e| {
            ExitError::new(
                GENERIC_ERROR,
                format!("Decrypted content is not valid UTF-8: {}", e),
                "",
            )
        })?;
    }
    clip.encrypted = false;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use client_core::protocol::Clip;

    #[derive(Parser)]
    #[command(no_binary_name = true)]
    struct ListArgsHarness {
        #[command(flatten)]
        args: Args,
    }

    #[test]
    fn args_text_only_sets_exclude_image() {
        let harness = ListArgsHarness::try_parse_from(["--text-only"]).expect("parse");
        let filter = build_filter(&harness.args, "remote:me", None);
        assert!(filter.exclude_image);
        assert!(!filter.exclude_text);
        assert!(filter.exclude_source.is_none());
        assert!(filter.source.is_none());
    }

    #[test]
    fn args_exclude_self_passes_self_key() {
        let harness = ListArgsHarness::try_parse_from(["--exclude-self"]).expect("parse");
        let filter = build_filter(&harness.args, "remote:me", None);
        assert_eq!(filter.exclude_source.as_deref(), Some("remote:me"));
    }

    #[test]
    fn args_from_passes_resolved_source() {
        let harness = ListArgsHarness::try_parse_from(["--from", "desktop"]).expect("parse");
        let filter = build_filter(&harness.args, "remote:me", Some("remote:desktop".into()));
        assert_eq!(filter.source.as_deref(), Some("remote:desktop"));
        assert!(filter.exclude_source.is_none());
    }

    #[test]
    fn resolve_source_matches_nickname_case_insensitive() {
        let dev = client_core::protocol::DeviceInfo {
            nickname: "Desktop".into(),
            hostname: "host-1".into(),
            source_key: "remote:dev-abc".into(),
            ..Default::default()
        };
        assert_eq!(resolve_source(&[dev], "desktop"), "remote:dev-abc");
    }

    #[test]
    fn resolve_source_falls_back_to_remote_prefix() {
        let devices: Vec<client_core::protocol::DeviceInfo> = vec![];
        assert_eq!(resolve_source(&devices, "ghost"), "remote:ghost");
    }

    fn text_clip(id: &str, src: &str, body: &str) -> Clip {
        Clip {
            clip_id: id.into(),
            content: body.into(),
            content_type: "text".into(),
            source: src.into(),
            byte_size: body.len() as i64,
            created_at: "2026-05-13T08:00:00Z".into(),
            ..Default::default()
        }
    }

    #[test]
    fn render_inlines_small_text() {
        let clip = text_clip("c1", "remote:desktop", "hello");
        let rec = render_record(&clip, "desktop");
        assert_eq!(rec.content.as_deref(), Some("hello"));
        assert_eq!(rec.preview, "hello");
        assert!(rec.image_metadata.is_none());
    }

    #[test]
    fn render_drops_content_above_threshold() {
        let big = "x".repeat(LIST_INLINE_LIMIT_BYTES + 1);
        let clip = text_clip("c2", "remote:desktop", &big);
        let rec = render_record(&clip, "desktop");
        assert!(rec.content.is_none());
        assert_eq!(rec.preview.chars().count(), PREVIEW_MAX_CHARS);
    }

    #[test]
    fn render_image_metadata_present() {
        let mut clip = text_clip("c3", "remote:phone", "");
        clip.content_type = "image".into();
        clip.byte_size = 1_200_000;
        let rec = render_record(&clip, "iphone");
        assert!(rec.content.is_none());
        assert_eq!(
            rec.image_metadata.as_ref().map(|m| m.mime.as_str()),
            Some("image")
        );
        assert!(rec.preview.starts_with("[image"));
    }

    #[test]
    fn render_table_aligns_columns() {
        let recs = vec![ListRecord {
            id: "c1".into(),
            source: "remote:desktop".into(),
            source_name: "desktop".into(),
            content_type: "text".into(),
            size_bytes: 142,
            created_at: "2026-05-13T08:00:00Z".into(),
            preview: "hello".into(),
            content: Some("hello".into()),
            image_metadata: None,
        }];
        // Pretend "now" is 5 minutes after the clip's created_at.
        let now = parse_rfc3339("2026-05-13T08:05:00Z").unwrap();
        let out = render_table(&recs, now);
        assert!(out.contains("5m"), "missing relative time: {}", out);
        assert!(out.contains("desktop"), "missing device name: {}", out);
        assert!(out.contains("text"), "missing type column: {}", out);
        assert!(out.contains("hello"), "missing preview: {}", out);
        // Verifies the layout: source(8) + 2sp + type(4) + 2sp + age(3) + 2sp + preview.
        assert_eq!(out, "desktop   text  5m   hello\n");
    }

    #[test]
    fn render_table_strips_remote_prefix_defensively() {
        // Local-store path passes raw `remote:hostname` as the source_name.
        let recs = vec![ListRecord {
            id: "c1".into(),
            source: "remote:laptop-7".into(),
            source_name: "remote:laptop-7".into(),
            content_type: "url".into(),
            size_bytes: 25,
            created_at: "2026-05-13T08:00:00Z".into(),
            preview: "https://example.com".into(),
            content: Some("https://example.com".into()),
            image_metadata: None,
        }];
        let now = parse_rfc3339("2026-05-13T09:00:00Z").unwrap();
        let out = render_table(&recs, now);
        assert!(!out.contains("remote:"), "remote: prefix leaked: {}", out);
        assert!(out.contains("laptop-7"), "device name missing: {}", out);
    }

    #[test]
    fn humanize_seconds_minutes_hours_days() {
        assert_eq!(humanize_duration(Duration::from_secs(30)), "30s");
        assert_eq!(humanize_duration(Duration::from_secs(120)), "2m");
        assert_eq!(humanize_duration(Duration::from_secs(7200)), "2h");
        assert_eq!(humanize_duration(Duration::from_secs(2 * 86_400)), "2d");
    }

    #[test]
    fn compact_type_maps_canonical_vocabulary() {
        assert_eq!(compact_type("text"), "text");
        assert_eq!(compact_type("code"), "code");
        assert_eq!(compact_type("url"), "url");
        assert_eq!(compact_type("image"), "img");
        // Legacy MIME strings (defensive) still detect canonically.
        assert_eq!(compact_type("image/png"), "img");
        assert_eq!(compact_type("text/plain"), "text");
        assert_eq!(compact_type("text/html"), "text");
        // Unknown vocab maps to '?' rather than leaking the raw value.
        assert_eq!(compact_type("audio"), "?");
        assert_eq!(compact_type(""), "?");
    }

    #[test]
    fn normalize_source_strips_remote_prefix() {
        assert_eq!(normalize_source("remote:desktop"), "desktop");
        assert_eq!(normalize_source("desktop"), "desktop");
        assert_eq!(normalize_source(""), "");
    }

    #[test]
    fn source_width_grows_with_longest_device() {
        let mk = |name: &str| ListRecord {
            id: "c".into(),
            source: name.into(),
            source_name: name.into(),
            content_type: "text".into(),
            size_bytes: 0,
            created_at: String::new(),
            preview: String::new(),
            content: None,
            image_metadata: None,
        };
        let recs = vec![mk("remote:tv"), mk("remote:very-long-device-name")];
        assert_eq!(
            source_width(&recs, 8),
            "very-long-device-name".chars().count()
        );
        assert_eq!(source_width(&[mk("remote:tv")], 8), 8);
    }
}
