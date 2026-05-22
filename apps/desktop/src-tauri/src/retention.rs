//! Local-history retention sweep. Purges clips older than the
//! `local_retention_days` setting (default 30 per D-05) on an hourly cadence.

use std::sync::Arc;

use crate::store;

/// Spawn the local retention sweep — purges clips older than the
/// configured `local_retention_days` setting (default 30 per D-05).
///
/// Cadence: hourly (D-06). Uses `MissedTickBehavior::Skip` so a laptop
/// that slept for 45 days does not trigger 45 back-to-back sweeps —
/// the next aligned tick suffices.
///
/// First tick fires immediately (tokio's documented behavior) — this
/// catches stale clips that accumulated while the app was quit longer
/// than the retention window. Intentional per RESEARCH.md Open Question 1.
pub(crate) fn spawn_retention_sweep(db: Arc<store::db::Database>) {
    tauri::async_runtime::spawn(async move {
        const DEFAULT_RETENTION_DAYS: i64 = 30;
        const SWEEP_INTERVAL_SECS: u64 = 60 * 60;

        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(SWEEP_INTERVAL_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        log::info!(
            "retention sweep started (interval = {}s, default = {}d)",
            SWEEP_INTERVAL_SECS,
            DEFAULT_RETENTION_DAYS,
        );

        loop {
            interval.tick().await; // first tick fires immediately — intentional
            let days = match db.get_setting("local_retention_days") {
                Ok(Some(v)) => v.parse::<i64>().unwrap_or(DEFAULT_RETENTION_DAYS),
                _ => DEFAULT_RETENTION_DAYS,
            };
            let cutoff = chrono::Utc::now().timestamp() - days * 86_400;
            match db.purge_before(cutoff) {
                Ok(n) if n > 0 => {
                    log::info!("retention sweep deleted {} clips older than {}d", n, days,)
                }
                Ok(_) => {}
                Err(e) => log::error!("retention sweep failed: {}", e),
            }
        }
    });
}
