//! `cinch device list` — list paired devices and source-only rows for this account.

use client_core::protocol::DeviceInfo;
use client_core::store::models::SourceRow;

use crate::exit::{ExitError, AUTH_FAILURE, GENERIC_ERROR};

fn cmp_devices_default(a: &DeviceInfo, b: &DeviceInfo) -> std::cmp::Ordering {
    b.online
        .cmp(&a.online)
        .then_with(|| b.last_push_at.cmp(&a.last_push_at))
}

fn print_names_dedup<I: IntoIterator<Item = String>>(names: I) {
    let mut seen = std::collections::HashSet::new();
    for name in names {
        if seen.insert(name.clone()) {
            println!("{}", name);
        }
    }
}

#[derive(Debug, clap::Args)]
pub struct Args {
    /// Print one name per line (nickname or hostname), no header.
    /// Intended for shell completion. Silent on auth/network failure.
    #[arg(long)]
    pub names: bool,
    /// Show only paired devices (legacy behavior). Default is the merged view.
    #[arg(long)]
    pub paired_only: bool,
}

pub async fn run(args: Args) -> Result<(), ExitError> {
    let ctx = match crate::runtime::open_ctx() {
        Ok(c) => c,
        Err(_) => {
            if args.names {
                return Ok(());
            }
            return Err(ExitError::new(
                AUTH_FAILURE,
                "No auth token configured.",
                "Run: cinch auth login",
            ));
        }
    };

    crate::runtime::opportunistic_backfill(&ctx).await;

    let mut paired = match ctx.client.list_devices().await {
        Ok(d) => d,
        Err(_) => {
            if args.names {
                return Ok(());
            }
            return Err(ExitError::new(
                GENERIC_ERROR,
                "Failed to fetch devices from relay.",
                "Check your network connection or run: cinch auth login",
            ));
        }
    };

    if args.paired_only {
        paired.sort_by(cmp_devices_default);
        if args.names {
            print_names_dedup(paired.iter().map(display_name));
            return Ok(());
        }
        if paired.is_empty() {
            eprintln!("No devices registered. Pair one with: cinch auth pair <token>");
            return Ok(());
        }
        print_table(&paired);
        return Ok(());
    }

    // --- Merged view ---

    // Fetch source-only rows from the local store.
    let sources = match client_core::store::queries::list_sources(&ctx.store) {
        Ok(s) => s,
        Err(e) => {
            return Err(ExitError::new(
                GENERIC_ERROR,
                format!("Failed to read local store: {}", e),
                "",
            ))
        }
    };
    let source_only: Vec<&SourceRow> = filter_source_only(&sources, &paired);

    if args.names {
        let mut seen = std::collections::HashSet::new();
        for d in &paired {
            let name = display_name(d);
            if seen.insert(name.clone()) {
                println!("{}", name);
            }
        }
        for s in &source_only {
            if seen.insert(s.source.clone()) {
                println!("{}", s.source);
            }
        }
        return Ok(());
    }

    let total = paired.len() + source_only.len();

    if total == 0 {
        eprintln!("No devices registered. Pair one with: cinch auth pair <token>");
        return Ok(());
    }

    println!("{} paired · {} total", paired.len(), total);

    if !paired.is_empty() {
        // Sort: online first, then most-recently-pushed first.
        paired.sort_by(cmp_devices_default);
        print_table(&paired);
    }

    // Source-only rows are already ordered by last_seen DESC NULLS LAST from list_sources.
    for s in &source_only {
        println!(
            "  {:<26}  not paired  last seen {}",
            s.source,
            crate::fmt::fmt_last_seen(s.last_seen)
        );
    }

    Ok(())
}

/// Returns the subset of `sources` whose `source` field does not match any
/// paired device's `hostname`. Preserves the input order (last_seen DESC).
pub fn filter_source_only<'a>(
    sources: &'a [SourceRow],
    paired: &[DeviceInfo],
) -> Vec<&'a SourceRow> {
    let paired_hosts: std::collections::HashSet<&str> =
        paired.iter().map(|d| d.hostname.as_str()).collect();
    sources
        .iter()
        .filter(|s| !paired_hosts.contains(s.source.as_str()))
        .collect()
}

fn display_name(d: &DeviceInfo) -> String {
    if d.nickname.is_empty() {
        d.hostname.clone()
    } else {
        d.nickname.clone()
    }
}

fn print_table(devices: &[DeviceInfo]) {
    const GUTTER: usize = 2;
    const DASH: &str = "—";

    let latest_cli = crate::update_check::cached_cli_latest();
    let version_cells: Vec<String> = devices
        .iter()
        .map(|d| render_version_cell(d, latest_cli.as_deref()))
        .collect();

    // Compute column widths.
    let name_w = devices
        .iter()
        .map(|d| display_name(d).len())
        .max()
        .unwrap_or(0)
        .max("NAME".len());
    let online_w = "ONLINE".len();
    let version_w = version_cells
        .iter()
        .map(|s| s.len())
        .max()
        .unwrap_or(0)
        .max("VERSION".len());
    let last_w = devices
        .iter()
        .map(|d| d.last_push_at.as_deref().unwrap_or(DASH).len())
        .max()
        .unwrap_or(0)
        .max("LAST PUSH".len());
    let clips_w = devices
        .iter()
        .map(|d| d.clip_count.to_string().len())
        .max()
        .unwrap_or(0)
        .max("CLIPS".len());

    // Header.
    println!(
        "  {:<nw$}{g}{:<ow$}{g}{:<vw$}{g}{:<lw$}{g}{:<cw$}",
        "NAME",
        "ONLINE",
        "VERSION",
        "LAST PUSH",
        "CLIPS",
        nw = name_w,
        g = " ".repeat(GUTTER),
        ow = online_w,
        vw = version_w,
        lw = last_w,
        cw = clips_w,
    );

    for (d, version) in devices.iter().zip(version_cells.iter()) {
        let name = display_name(d);
        let online = if d.online { "yes" } else { "no" };
        let last = d.last_push_at.as_deref().unwrap_or(DASH);
        let clips = d.clip_count.to_string();

        println!(
            "  {:<nw$}{g}{:<ow$}{g}{:<vw$}{g}{:<lw$}{g}{:<cw$}",
            name,
            online,
            version,
            last,
            clips,
            nw = name_w,
            g = " ".repeat(GUTTER),
            ow = online_w,
            vw = version_w,
            lw = last_w,
            cw = clips_w,
        );
    }
}

/// Renders the VERSION cell for a device row. Shows the reported
/// `client_version`, with " (outdated)" appended when the device is a
/// CLI and its reported version is strictly less than the cached latest
/// CLI tag. Desktop devices never get the marker — they self-update.
fn render_version_cell(d: &DeviceInfo, latest_cli: Option<&str>) -> String {
    const DASH: &str = "—";
    let Some(version) = d.client_version.as_deref() else {
        return DASH.to_string();
    };
    let is_cli = d.client_type.as_deref() == Some("cli");
    let outdated = is_cli
        && latest_cli
            .map(|l| crate::update_check::is_outdated(version, l))
            .unwrap_or(false);
    if outdated {
        format!("{} (outdated)", version)
    } else {
        version.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_core::protocol::DeviceInfo;
    use client_core::store::models::SourceRow;

    fn make_device(hostname: &str) -> DeviceInfo {
        DeviceInfo {
            hostname: hostname.to_string(),
            nickname: String::new(),
            online: false,
            last_push_at: None,
            clip_count: 0,
            ..Default::default()
        }
    }

    fn make_source(source: &str, last_seen: Option<i64>) -> SourceRow {
        SourceRow {
            source: source.to_string(),
            clip_count: 0,
            last_seen,
        }
    }

    #[test]
    fn test_filter_source_only_excludes_paired_hostnames() {
        let paired = vec![make_device("laptop"), make_device("desktop")];
        let sources = vec![
            make_source("laptop", Some(1000)), // already paired — must be excluded
            make_source("desktop", Some(2000)), // already paired — must be excluded
            make_source("ci-runner", Some(3000)), // source-only — must be included
            make_source("old-box", None),      // source-only — must be included
        ];

        let result = filter_source_only(&sources, &paired);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].source, "ci-runner");
        assert_eq!(result[1].source, "old-box");
    }

    #[test]
    fn test_fmt_last_seen_none_returns_dash() {
        assert_eq!(crate::fmt::fmt_last_seen(None), "—");
    }

    #[test]
    fn test_fmt_last_seen_epoch_zero_is_rfc3339() {
        let result = crate::fmt::fmt_last_seen(Some(0));
        assert_eq!(result, "1970-01-01T00:00:00Z");
    }

    fn make_device_versioned(version: Option<&str>, client_type: Option<&str>) -> DeviceInfo {
        DeviceInfo {
            hostname: "host".to_string(),
            nickname: String::new(),
            online: false,
            last_push_at: None,
            clip_count: 0,
            client_version: version.map(|s| s.to_string()),
            client_type: client_type.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn render_version_cell_dash_when_unreported() {
        let d = make_device_versioned(None, None);
        assert_eq!(render_version_cell(&d, Some("v0.1.8")), "—");
    }

    #[test]
    fn render_version_cell_plain_for_desktop_even_if_lower() {
        // Desktop self-updates; we never mark its row outdated.
        let d = make_device_versioned(Some("0.1.5"), Some("desktop"));
        assert_eq!(render_version_cell(&d, Some("v0.1.8")), "0.1.5");
    }

    #[test]
    fn render_version_cell_outdated_for_cli_below_latest() {
        let d = make_device_versioned(Some("0.1.5"), Some("cli"));
        assert_eq!(render_version_cell(&d, Some("v0.1.8")), "0.1.5 (outdated)");
    }

    #[test]
    fn render_version_cell_plain_for_cli_at_or_above_latest() {
        let d = make_device_versioned(Some("0.1.8"), Some("cli"));
        assert_eq!(render_version_cell(&d, Some("v0.1.8")), "0.1.8");
        let d2 = make_device_versioned(Some("0.2.0"), Some("cli"));
        assert_eq!(render_version_cell(&d2, Some("v0.1.8")), "0.2.0");
    }

    #[test]
    fn render_version_cell_plain_when_no_cached_latest() {
        let d = make_device_versioned(Some("0.1.5"), Some("cli"));
        assert_eq!(render_version_cell(&d, None), "0.1.5");
    }
}
