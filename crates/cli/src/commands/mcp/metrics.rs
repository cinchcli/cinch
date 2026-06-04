//! Fleet-read validation-gate metrics (send/fleet-read spec §7, item 2).
//!
//! The MCP server runs on the quiet stdio path — no telemetry client, no async
//! runtime, and any stray stdout would corrupt the JSON-RPC stream. So it
//! CANNOT emit telemetry directly. Instead it accumulates `scope:"fleet"` call
//! counts in-process and writes them to a per-session counter file under
//! `~/.cinch/mcp-metrics/`, **flushed incrementally** (MCP processes are almost
//! always SIGKILL/SIGTERM'd, so a flush-only-on-EOF scheme would lose the gate
//! metric). The next ordinary `cinch` invocation — which has telemetry and a
//! runtime — drains those files via [`drain_and_emit`], emits one
//! `mcp.session.completed` per session, and deletes them. To keep that
//! one-per-session invariant, drain skips files a live session may still be
//! rewriting (a grace window) and claims each file with an atomic rename
//! before emitting, so concurrent `cinch` invocations can't double-count.
//!
//! Every filesystem operation here is **best-effort**: on a read-only agent box
//! the writes fail silently, losing only the gate metric, never affecting the
//! read. This module is the only thing that can fail, and it is built so it
//! cannot fail the serve loop.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{json, Value};

/// How long a counter file must sit untouched before [`drain_and_emit`] will
/// claim it. A live MCP session rewrites its file on every fleet call, so
/// skipping recently-modified files keeps an ordinary `cinch` invocation from
/// draining (and double-counting) a session that is still running. The gate
/// metric is an aggregate validation signal, not real-time, so a minute of
/// emission lag is harmless. Residual: a live session idle *longer* than this
/// window that then resumes can still be drained mid-flight — far narrower than
/// the unconditional drain it replaces, and it only ever skews this internal
/// metric, never a read.
const DRAIN_GRACE: Duration = Duration::from_secs(60);

/// `~/.cinch/mcp-metrics/` (the directory the counter files live in).
fn metrics_dir() -> Option<PathBuf> {
    let db = client_core::store::default_db_path().ok()?;
    Some(db.parent()?.join("mcp-metrics"))
}

/// In-process accumulator for one MCP serve session. Updated per request and
/// rewritten to its own counter file on each fleet call (low-volume) + a final
/// flush at clean EOF.
pub struct FleetMetrics {
    all_calls: u64,
    fleet_calls: u64,
    fleet_rows: u64,
    /// Session start (epoch ms); also the `ts` field and part of the filename.
    started_ms: i64,
    /// Counter-file path; `None` when the metrics dir can't be resolved (e.g.
    /// no HOME) — the whole accumulator then degrades to a silent no-op.
    file: Option<PathBuf>,
}

impl FleetMetrics {
    pub fn new(started_ms: i64) -> Self {
        let file = metrics_dir().map(|dir| {
            let pid = std::process::id();
            dir.join(format!("mcp-{pid}-{started_ms}.json"))
        });
        Self {
            all_calls: 0,
            fleet_calls: 0,
            fleet_rows: 0,
            started_ms,
            file,
        }
    }

    /// Record one dispatched request. Counts `tools/call`s, and for
    /// `scope:"fleet"` calls also the rows returned (parsed from the response
    /// envelope). Flushes incrementally on each fleet call. Never panics.
    pub fn record(&mut self, msg: &Value, response: &Option<Value>) {
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        if method != "tools/call" {
            return;
        }
        self.all_calls += 1;

        let is_fleet = msg
            .get("params")
            .and_then(|p| p.get("arguments"))
            .and_then(|a| a.get("scope"))
            .and_then(Value::as_str)
            == Some("fleet");
        if !is_fleet {
            return;
        }
        self.fleet_calls += 1;
        self.fleet_rows += rows_in_response(response);
        // Incremental flush: fleet calls are rare, so flushing on each one is
        // cheap and survives the usual SIGKILL.
        self.flush();
    }

    /// Final flush (clean EOF). Correctness does not depend on reaching it —
    /// the per-fleet-call flushes already persisted the gate metric.
    pub fn finalize(&self) {
        // Only worth a final write if at least one tool call happened.
        if self.all_calls > 0 {
            self.flush();
        }
    }

    /// Rewrite the session counter file with the current running totals.
    /// Best-effort: any IO error is swallowed.
    fn flush(&self) {
        let Some(path) = &self.file else {
            return;
        };
        let Some(dir) = path.parent() else {
            return;
        };
        if std::fs::create_dir_all(dir).is_err() {
            return; // read-only / unwritable — drop the metric, never the read.
        }
        let line = json!({
            "ts": self.started_ms,
            "fleet_calls": self.fleet_calls,
            "fleet_rows": self.fleet_rows,
            "all_calls": self.all_calls,
        });
        let _ = std::fs::write(path, format!("{line}\n"));
    }
}

/// Count the rows in a fleet tool-call response. The list/search tools wrap a
/// JSON array in the MCP text envelope (`result.content[0].text`); anything
/// else (errors, the no-scope `get_clipboard_item`) counts as 0.
fn rows_in_response(response: &Option<Value>) -> u64 {
    let Some(resp) = response else {
        return 0;
    };
    let Some(text) = resp
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
    else {
        return 0;
    };
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Array(a)) => a.len() as u64,
        _ => 0,
    }
}

/// Drain every MCP session counter file: emit one `mcp.session.completed`
/// telemetry event per file, then delete it. Called from the instrumented CLI
/// path (telemetry initialized, async runtime present). Best-effort — swallows
/// all IO/parse errors so a malformed or unreadable file never affects the
/// invoking command.
pub fn drain_and_emit() {
    let Some(dir) = metrics_dir() else {
        return;
    };
    drain_dir(&dir, SystemTime::now(), emit_session_event);
}

/// Core drain loop, parameterized for testing (`now` is injected so the grace
/// window can be exercised deterministically; `emit` captures the events).
///
/// Two guards make this exactly-once for the common lifecycle:
/// 1. **Grace window** — skip files modified within [`DRAIN_GRACE`], so a live
///    session still rewriting its file isn't drained out from under it.
/// 2. **Atomic claim** — rename each eligible file to a sibling the `*.json`
///    scan won't match before reading it, so two concurrent `cinch`
///    invocations can't both emit the same session (only one rename wins).
fn drain_dir(dir: &Path, now: SystemTime, mut emit: impl FnMut(&Value)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return; // dir absent (no MCP session ever ran) — nothing to drain.
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        // Only positively skip when we can prove the file is recent; on any
        // metadata error fall through and drain so the metric isn't lost.
        if let Ok(modified) = entry.metadata().and_then(|m| m.modified()) {
            if let Ok(age) = now.duration_since(modified) {
                if age < DRAIN_GRACE {
                    continue; // a live session may still own it.
                }
            }
        }
        // Claim the file atomically: the loser of a rename race (or a vanished
        // file) just skips. The `.draining` sibling is invisible to the
        // `*.json` scan above, so no other drainer re-picks it.
        let claimed = path.with_extension("draining");
        if std::fs::rename(&path, &claimed).is_err() {
            continue;
        }
        if let Ok(content) = std::fs::read_to_string(&claimed) {
            for line in content.lines().filter(|l| !l.trim().is_empty()) {
                if let Ok(v) = serde_json::from_str::<Value>(line) {
                    emit(&v);
                }
            }
        }
        let _ = std::fs::remove_file(&claimed);
    }
}

fn emit_session_event(v: &Value) {
    let fleet_calls = v.get("fleet_calls").and_then(Value::as_u64).unwrap_or(0);
    let fleet_rows = v.get("fleet_rows").and_then(Value::as_u64).unwrap_or(0);
    let all_calls = v.get("all_calls").and_then(Value::as_u64).unwrap_or(0);
    crate::telemetry::capture(
        crate::telemetry::Event::new("mcp.session.completed")
            .with("fleet_calls", fleet_calls)
            .with("fleet_rows", fleet_rows)
            .with("all_calls", all_calls)
            // The loop-completion gate (§7 item 3): did this session read at
            // least one fleet clip?
            .with("fleet_rows_present", fleet_rows > 0),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_ignores_non_tool_calls() {
        let mut m = FleetMetrics::new(1000);
        m.file = None; // disable IO for the unit test
        let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
        m.record(&init, &None);
        assert_eq!(m.all_calls, 0);
        assert_eq!(m.fleet_calls, 0);
    }

    #[test]
    fn record_counts_all_and_fleet_calls() {
        let mut m = FleetMetrics::new(1000);
        m.file = None;
        // A scope:"all" tool call: counts toward all_calls only.
        let all = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"list_recent_clipboard","arguments":{"scope":"all"}}
        });
        m.record(
            &all,
            &Some(json!({"result":{"content":[{"type":"text","text":"[]"}]}})),
        );
        assert_eq!(m.all_calls, 1);
        assert_eq!(m.fleet_calls, 0);
        assert_eq!(m.fleet_rows, 0);

        // A scope:"fleet" call returning two rows.
        let fleet = json!({
            "jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"list_recent_clipboard","arguments":{"scope":"fleet"}}
        });
        let resp = json!({"result":{"content":[{"type":"text","text":"[{\"id\":\"a\"},{\"id\":\"b\"}]"}]}});
        m.record(&fleet, &Some(resp));
        assert_eq!(m.all_calls, 2);
        assert_eq!(m.fleet_calls, 1);
        assert_eq!(m.fleet_rows, 2);
    }

    #[cfg(unix)]
    #[test]
    fn flush_on_readonly_dir_is_swallowed() {
        // §9: a read-only metrics dir must NOT fail the read — flush swallows
        // the IO error and never panics; counters still advance in-process.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555))
            .expect("chmod read-only");

        let mut m = FleetMetrics::new(1000);
        m.file = Some(dir.path().join("mcp-test.json"));
        let fleet = json!({
            "jsonrpc":"2.0","id":1,"method":"tools/call",
            "params":{"name":"list_recent_clipboard","arguments":{"scope":"fleet"}}
        });
        let resp = json!({"result":{"content":[{"type":"text","text":"[1]"}]}});
        // record() flushes internally; must not panic on the read-only dir.
        m.record(&fleet, &Some(resp));
        assert_eq!(
            m.fleet_calls, 1,
            "counter still advances despite failed write"
        );
        assert!(
            !dir.path().join("mcp-test.json").exists(),
            "no file should be written to a read-only dir"
        );

        // Restore perms so the tempdir can be cleaned up.
        let _ = std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755));
    }

    #[test]
    fn rows_in_response_handles_shapes() {
        assert_eq!(rows_in_response(&None), 0);
        // Error envelope (no result) → 0.
        assert_eq!(rows_in_response(&Some(json!({"error":{"code":-32000}}))), 0);
        // get_clipboard_item returns a single object, not an array → 0.
        let obj = json!({"result":{"content":[{"type":"text","text":"{\"id\":\"x\"}"}]}});
        assert_eq!(rows_in_response(&Some(obj)), 0);
        // Array of three → 3.
        let arr = json!({"result":{"content":[{"type":"text","text":"[1,2,3]"}]}});
        assert_eq!(rows_in_response(&Some(arr)), 3);
    }

    /// Write a realistic per-session counter file via the same flush path the
    /// serve loop uses.
    fn write_counter_file(dir: &Path, started_ms: i64, fleet_calls: u64, fleet_rows: u64) {
        let mut m = FleetMetrics::new(started_ms);
        m.file = Some(dir.join(format!("mcp-test-{started_ms}.json")));
        m.all_calls = fleet_calls;
        m.fleet_calls = fleet_calls;
        m.fleet_rows = fleet_rows;
        m.finalize(); // all_calls > 0 → writes the counter file
    }

    #[test]
    fn drain_skips_recently_written_files() {
        // A file written "just now" is within the grace window: a live session
        // may still own it, so drain must leave it untouched.
        let dir = tempfile::tempdir().expect("tempdir");
        write_counter_file(dir.path(), 1000, 2, 5);
        let mut emitted = Vec::new();
        drain_dir(dir.path(), SystemTime::now(), |v| emitted.push(v.clone()));
        assert!(emitted.is_empty(), "recent file must not be drained");
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "skipped file is left in place for a later drain"
        );
    }

    #[test]
    fn drain_emits_and_removes_stale_files() {
        // A file past the grace window (simulated via a future `now`) is drained
        // exactly once and removed.
        let dir = tempfile::tempdir().expect("tempdir");
        write_counter_file(dir.path(), 1000, 2, 5);
        let future = SystemTime::now() + DRAIN_GRACE * 4;
        let mut emitted = Vec::new();
        drain_dir(dir.path(), future, |v| emitted.push(v.clone()));
        assert_eq!(emitted.len(), 1, "one event per stale session file");
        assert_eq!(emitted[0]["fleet_calls"].as_u64(), Some(2));
        assert_eq!(emitted[0]["fleet_rows"].as_u64(), Some(5));
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            0,
            "drained file (and its .draining claim) are removed"
        );
    }

    #[test]
    fn drain_claims_each_file_once() {
        // The atomic claim makes a second drain pass a no-op: the session is
        // emitted exactly once even if two `cinch` invocations race over it.
        let dir = tempfile::tempdir().expect("tempdir");
        write_counter_file(dir.path(), 1000, 1, 3);
        let future = SystemTime::now() + DRAIN_GRACE * 4;
        let mut emitted = Vec::new();
        drain_dir(dir.path(), future, |v| emitted.push(v.clone()));
        drain_dir(dir.path(), future, |v| emitted.push(v.clone()));
        assert_eq!(emitted.len(), 1, "session emitted once across two drains");
    }
}
