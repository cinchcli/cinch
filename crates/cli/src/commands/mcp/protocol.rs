//! Newline-delimited JSON-RPC 2.0 router for the MCP stdio transport.

use crate::exit::ExitError;
use client_core::store::Store;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const PROTOCOL_VERSION: &str = "2024-11-05";

/// Read newline-delimited JSON-RPC messages from stdin, dispatch each, and
/// write responses (one JSON object per line) to stdout. Returns on EOF.
pub fn serve_stdio(store: &Store) -> Result<(), ExitError> {
    // Exposure-scope cutoff, computed once at startup (opt-in privacy lever).
    let max_age =
        super::query::parse_max_age_days(std::env::var("CINCH_MCP_MAX_AGE_DAYS").ok().as_deref());
    let since_ms = super::query::since_ms_from_days(chrono::Utc::now().timestamp_millis(), max_age);

    // "This device" identity, computed ONCE (network-free, synchronous). Threaded
    // into the handler so the fleet exclude-self predicate is deterministic and
    // unit-testable with an injected value (§4.3).
    let self_source = client_core::machine::self_source_key();

    // Process-lifetime once-guard for the lazy fleet backfill (§4.1.1). The first
    // `scope:"fleet"` tool call (when CINCH_MCP_FLEET=1) flips this; it never
    // re-runs in the same session — even if the backfill itself errored.
    let mut fleet_backfill_done = false;

    // Validation-gate counter (§7 item 2). Best-effort, never touches stdout;
    // drained by the next ordinary `cinch` invocation. See `metrics.rs`.
    let mut metrics = super::metrics::FleetMetrics::new(chrono::Utc::now().timestamp_millis());

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| {
            ExitError::new(
                crate::exit::GENERIC_ERROR,
                format!("stdin read failed: {e}"),
                "",
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let parsed = serde_json::from_str::<Value>(&line);
        let response = match &parsed {
            Ok(msg) => {
                // Lazy fleet backfill: runs AFTER this request line is fully read
                // and BEFORE its response is written, so stdout stays silent while
                // it blocks (the stdio JSON-RPC contract). At most once per session.
                maybe_fleet_backfill(store, &self_source, msg, &mut fleet_backfill_done);
                handle_request(store, since_ms, &self_source, msg)
            }
            // Parse error: reply per JSON-RPC with a null id.
            Err(e) => Some(json!({
                "jsonrpc": "2.0", "id": Value::Null,
                "error": { "code": -32700, "message": format!("parse error: {e}") }
            })),
        };
        // Record the gate metric (best-effort) before writing the response.
        if let Ok(msg) = &parsed {
            metrics.record(msg, &response);
        }
        if let Some(resp) = response {
            let line = serde_json::to_string(&resp).map_err(|e| {
                ExitError::new(
                    crate::exit::GENERIC_ERROR,
                    format!("serialize failed: {e}"),
                    "",
                )
            })?;
            writeln!(stdout, "{line}").map_err(|e| {
                ExitError::new(
                    crate::exit::GENERIC_ERROR,
                    format!("stdout write failed: {e}"),
                    "",
                )
            })?;
            stdout.flush().ok();
        }
    }
    // Clean EOF: final flush. Correctness does not depend on reaching this —
    // the per-fleet-call flushes already persisted the gate metric.
    metrics.finalize();
    Ok(())
}

/// Pure decision predicate for the lazy fleet backfill (testable without a relay).
///
/// Returns `true` iff ALL trigger conditions hold (§4.1.1):
/// - `CINCH_MCP_FLEET=1` is set (`flag_enabled`) — unset => NEVER (pure-local default);
/// - `!done` — the once-guard has not already fired;
/// - `msg` is a `method == "tools/call"` whose `params.arguments.scope`, after the
///   `"local"`/unknown => `"all"` normalization, equals `"fleet"`.
///
/// `initialize`, `ping`, `tools/list`, and `scope == "all"`/`"local"`/absent never trigger.
fn fleet_backfill_should_trigger(flag_enabled: bool, done: bool, msg: &Value) -> bool {
    if !flag_enabled || done {
        return false;
    }
    if msg.get("method").and_then(Value::as_str) != Some("tools/call") {
        return false;
    }
    let args = match msg.get("params").and_then(|p| p.get("arguments")) {
        Some(a) => a,
        None => return false,
    };
    scope_is_fleet(args)
}

/// Lazy, once-guarded fleet backfill (§4.1.1) — the ONLY network this MCP process
/// ever performs. Runs at most once per session, on the first `scope:"fleet"`
/// tool call, gated by `CINCH_MCP_FLEET=1`. Called AFTER the request line is read
/// and BEFORE its response is written, so stdout stays silent while it blocks.
///
/// All errors are swallowed: a relay-unreachable box just serves stale-but-local
/// data. The once-flag flips even on error, so the backfill never repeats.
fn maybe_fleet_backfill(store: &Store, _self_source: &str, msg: &Value, done: &mut bool) {
    let flag_enabled = std::env::var("CINCH_MCP_FLEET").as_deref() == Ok("1");
    if !fleet_backfill_should_trigger(flag_enabled, *done, msg) {
        return;
    }
    // Set the once-flag BEFORE running so it is marked done even if the backfill
    // panics or errors — it must run at most once per session.
    *done = true;
    run_fleet_backfill(store);
}

/// Synchronous one-shot REST backfill, built and dropped entirely within this call.
///
/// Loads credentials synchronously (network-free). If the token is empty, skips
/// silently (not authed). Otherwise builds a current-thread tokio runtime (same
/// idiom as `lib.rs`) and `block_on`s a `backfill_once` wrapped in a 2s timeout.
///
/// This calls `backfill_once` DIRECTLY, bypassing `runtime::opportunistic_backfill`'s
/// lockfile (report F6). Concurrent-writer safety then rests on SQLite WAL +
/// `busy_timeout=5000` + `insert_clip`'s id-idempotency. No stdout is touched here;
/// any diagnostic would go to stderr only (kept silent by default).
fn run_fleet_backfill(store: &Store) {
    let cfg = match client_core::auth::load_config() {
        Ok(c) => c,
        Err(_) => return, // no config => nothing to sync against; serve local.
    };
    if cfg.token.is_empty() {
        return; // not authenticated — skip silently, serve already-synced rows.
    }

    let client = match client_core::http::RestClient::new(
        cfg.relay_url.clone(),
        cfg.token.clone(),
        crate::client_info::for_cli(),
    ) {
        Ok(c) => c,
        Err(_) => return,
    };
    let enc_key = client_core::credstore::read_encryption_key(&cfg.user_id);

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return,
    };
    rt.block_on(async {
        // 2s timeout so a slow relay cannot delay the first fleet call's response
        // indefinitely. All errors (and the timeout) are swallowed.
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client_core::sync::backfill_once(
                store,
                &client,
                client_core::sync::BackfillBudget::default(),
                enc_key.as_ref(),
            ),
        )
        .await;
    });
    // The runtime is dropped here, before the fleet call's response is written.
}

/// Handle one JSON-RPC message. Returns `Some(response)` for requests and
/// `None` for notifications (messages without an `id`, which must not be answered).
/// `since_ms` is the exposure-scope cutoff (None = full history); it bounds what
/// the tool calls return. `self_source` is this device's source key (§4.3),
/// used to exclude this machine's own clips on a `scope:"fleet"` read.
pub fn handle_request(
    store: &Store,
    since_ms: Option<i64>,
    self_source: &str,
    msg: &Value,
) -> Option<Value> {
    let id = msg.get("id").cloned()?; // notifications carry no id
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "cinch", "version": env!("CARGO_PKG_VERSION") }
        })),
        "tools/list" => Ok(tools_list()),
        "tools/call" => handle_tool_call(store, since_ms, self_source, msg.get("params")),
        "ping" => Ok(json!({})),
        other => Err((-32601, format!("method not found: {other}"))),
    };

    Some(match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
        Err((code, message)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
        }
    })
}

fn tools_list() -> Value {
    json!({ "tools": [
        {
            "name": "search_clipboard",
            "description": "Full-text search the user's local clipboard history. Returns matching clips as previews; use get_clipboard_item for full content. Results may be fewer than `limit` when CINCH_MCP_MAX_AGE_DAYS is set.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Natural-language or keyword query." },
                    "limit": { "type": "integer", "description": "Max results (default 20, max 100)." },
                    "type": { "type": "string", "description": "Optional type filter (text, image, url, code)." },
                    "scope": {
                        "type": "string",
                        "enum": ["all", "fleet"],
                        "description": "'all' (default) = every local clip incl. remote-origin; 'fleet' = only clips from OTHER machines (this device excluded). 'local' is accepted as a deprecated alias for 'all'."
                    }
                },
                "required": ["query"]
            }
        },
        {
            "name": "list_recent_clipboard",
            "description": "List the most recent local clipboard items. limit=1 returns what was just copied.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max results (default 20, max 100)." },
                    "source": { "type": "string", "description": "Optional source filter, e.g. a device id." },
                    "scope": {
                        "type": "string",
                        "enum": ["all", "fleet"],
                        "description": "'all' (default) = every local clip incl. remote-origin; 'fleet' = only clips from OTHER machines (this device excluded). 'local' is accepted as a deprecated alias for 'all'."
                    }
                }
            }
        },
        {
            "name": "get_clipboard_item",
            "description": "Get the full content of one clipboard item by id. Image items return metadata only.",
            "inputSchema": {
                "type": "object",
                "properties": { "id": { "type": "string" } },
                "required": ["id"]
            }
        }
    ]})
}

use super::mapping::to_mcp_clip;
use super::query::clamp_limit;
use client_core::store::queries::sanitize_fts_query;

/// Normalize the optional `scope` argument into the canonical two-value set.
///
/// `"fleet"` stays `"fleet"`; the deprecated alias `"local"` maps to `"all"`;
/// absent, `"all"`, or ANY unknown value falls back to `"all"` (defensive — an
/// unrecognized scope must never silently leak this machine's own clips out, but
/// it also must never error a read). Returns `true` iff the effective scope is
/// `"fleet"`.
fn scope_is_fleet(args: &Value) -> bool {
    let scope = args.get("scope").and_then(Value::as_str).unwrap_or("all");
    matches!(scope, "fleet")
}

fn handle_tool_call(
    store: &Store,
    since_ms: Option<i64>,
    self_source: &str,
    params: Option<&Value>,
) -> Result<Value, (i64, String)> {
    let params = params.ok_or((-32602, "missing params".to_string()))?;
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    // Exposure-scope: keep only clips at or newer than the cutoff (None = all).
    let within = |created_at: i64| since_ms.is_none_or(|s| created_at >= s);

    let payload: Value = match name {
        "search_clipboard" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or((-32602, "missing required 'query'".to_string()))?;
            let limit = clamp_limit(args.get("limit").and_then(Value::as_i64));
            let filter_type = args.get("type").and_then(Value::as_str);
            // scope:"fleet" => exclude this machine's own clips; ANDs with since_ms.
            let exclude_source = if scope_is_fleet(&args) {
                Some(self_source)
            } else {
                None
            };

            let fts = sanitize_fts_query(query);
            // The empty-query path goes through list_clips (DB-filters by since_ms);
            // the FTS path uses search_clips (no since arg) so we filter in Rust.
            let rows = if fts.is_empty() {
                client_core::store::queries::list_clips(
                    store,
                    None,
                    exclude_source,
                    Some(limit),
                    None,
                    since_ms,
                    false,
                    limit,
                )
            } else {
                client_core::store::queries::search_clips(
                    store,
                    query,
                    limit,
                    filter_type,
                    exclude_source,
                )
            }
            .map_err(|e| (-32000, format!("store error: {e}")))?;
            let clips: Vec<_> = rows
                .iter()
                .filter(|c| within(c.created_at))
                .map(|c| to_mcp_clip(c, false))
                .collect();
            serde_json::to_value(clips).unwrap_or_else(|_| json!([]))
        }
        "list_recent_clipboard" => {
            let limit = clamp_limit(args.get("limit").and_then(Value::as_i64));
            let source = args.get("source").and_then(Value::as_str);
            // scope:"fleet" => exclude this machine's own clips; ANDs with since_ms.
            let exclude_source = if scope_is_fleet(&args) {
                Some(self_source)
            } else {
                None
            };
            let rows = client_core::store::queries::list_clips(
                store,
                source,
                exclude_source,
                Some(limit),
                None,
                since_ms,
                false,
                limit,
            )
            .map_err(|e| (-32000, format!("store error: {e}")))?;
            let clips: Vec<_> = rows.iter().map(|c| to_mcp_clip(c, false)).collect();
            serde_json::to_value(clips).unwrap_or_else(|_| json!([]))
        }
        "get_clipboard_item" => {
            let id = args.get("id").and_then(Value::as_str).unwrap_or("");
            let found = client_core::store::queries::get_clip(store, id)
                .map_err(|e| (-32000, format!("store error: {e}")))?;
            match found {
                Some(c) if within(c.created_at) => {
                    serde_json::to_value(to_mcp_clip(&c, true)).unwrap_or(Value::Null)
                }
                _ => Value::Null,
            }
        }
        other => return Err((-32601, format!("unknown tool: {other}"))),
    };

    // MCP tool-result envelope: a single text block holding the JSON payload.
    Ok(json!({
        "content": [ { "type": "text", "text": serde_json::to_string(&payload).unwrap_or_default() } ]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use client_core::store::Store;
    use std::path::Path;

    /// Deterministic self-source injected into the handler in tests, so the
    /// fleet exclude-self predicate does not depend on the real hostname (§4.3).
    const TEST_SELF: &str = "remote:self-host";

    fn mem_store() -> Store {
        Store::open(Path::new(":memory:")).expect("open in-memory store")
    }

    /// Dispatch one JSON-RPC message through `handle_request`, threading the
    /// deterministic `TEST_SELF` source key.
    fn dispatch(store: &Store, since_ms: Option<i64>, req: &Value) -> Option<Value> {
        handle_request(store, since_ms, TEST_SELF, req)
    }

    #[test]
    fn initialize_reports_server_info() {
        let store = mem_store();
        let req = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
        let resp = dispatch(&store, None, &req).expect("response");
        assert_eq!(resp["result"]["serverInfo"]["name"], "cinch");
        assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
    }

    #[test]
    fn tools_list_has_three_read_tools() {
        let store = mem_store();
        let req = json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}});
        let resp = dispatch(&store, None, &req).expect("response");
        let names: Vec<&str> = resp["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "search_clipboard",
                "list_recent_clipboard",
                "get_clipboard_item"
            ]
        );
    }

    #[test]
    fn notification_without_id_gets_no_reply() {
        let store = mem_store();
        let req = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        assert!(dispatch(&store, None, &req).is_none());
    }

    #[test]
    fn unknown_method_is_jsonrpc_error() {
        let store = mem_store();
        let req = json!({"jsonrpc":"2.0","id":3,"method":"bogus"});
        let resp = dispatch(&store, None, &req).expect("response");
        assert_eq!(resp["error"]["code"], -32601);
    }

    use client_core::store::models::StoredClip;
    use client_core::store::queries::insert_clip;

    fn seed(store: &Store, id: &str, content_type: &str, content: &str, created_at: i64) {
        seed_with_source(
            store,
            id,
            "remote:macbook",
            content_type,
            content,
            created_at,
        );
    }

    fn seed_with_source(
        store: &Store,
        id: &str,
        source: &str,
        content_type: &str,
        content: &str,
        created_at: i64,
    ) {
        insert_clip(
            store,
            &StoredClip {
                id: id.to_string(),
                source: source.to_string(),
                source_key: None,
                source_app_id: None,
                source_app: None,
                source_url: None,
                label: None,
                content_type: content_type.to_string(),
                content: Some(content.as_bytes().to_vec()),
                media_path: None,
                byte_size: content.len() as i64,
                created_at,
                pinned: false,
                pinned_at: None,
                sync_state: client_core::store::models::SyncState::Synced,
            },
        )
        .expect("insert");
    }

    fn call(store: &Store, name: &str, args: Value) -> Value {
        call_since(store, None, name, args)
    }

    fn call_since(store: &Store, since_ms: Option<i64>, name: &str, args: Value) -> Value {
        let req = json!({"jsonrpc":"2.0","id":9,"method":"tools/call",
                         "params":{"name":name,"arguments":args}});
        let resp = dispatch(store, since_ms, &req).expect("response");
        // Tool result envelope: content[0].text is a JSON string of the clip array.
        let text = resp["result"]["content"][0]["text"].as_str().expect("text");
        serde_json::from_str(text).expect("json array")
    }

    /// Extract the `id` field of every clip in a returned array, in order.
    fn ids_of(out: &Value) -> Vec<String> {
        out.as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn search_finds_seeded_clip() {
        let store = mem_store();
        seed(
            &store,
            "01A",
            "text",
            "the quick brown fox",
            1_700_000_000_000,
        );
        seed(
            &store,
            "01B",
            "text",
            "totally unrelated",
            1_700_000_000_001,
        );
        let out: Value = call(&store, "search_clipboard", json!({"query":"brown fox"}));
        let ids: Vec<&str> = out
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        assert!(ids.contains(&"01A"));
        assert!(!ids.contains(&"01B"));
    }

    #[test]
    fn search_with_punctuation_does_not_error() {
        let store = mem_store();
        seed(
            &store,
            "01A",
            "text",
            "error: NPE at foo-bar",
            1_700_000_000_000,
        );
        // Raw FTS would choke on `:` and `-`; sanitizer must keep it a valid call.
        let out: Value = call(
            &store,
            "search_clipboard",
            json!({"query":"error: foo-bar"}),
        );
        assert!(out.is_array());
    }

    #[test]
    fn list_recent_orders_newest_first() {
        let store = mem_store();
        seed(&store, "old", "text", "old", 1_700_000_000_000);
        seed(&store, "new", "text", "new", 1_700_000_009_999);
        let out: Value = call(&store, "list_recent_clipboard", json!({"limit":1}));
        assert_eq!(out.as_array().unwrap().len(), 1);
        assert_eq!(out[0]["id"], "new");
    }

    #[test]
    fn get_item_returns_full_content_or_null() {
        let store = mem_store();
        seed(
            &store,
            "01A",
            "text",
            "full content here",
            1_700_000_000_000,
        );
        let found: Value = call(&store, "get_clipboard_item", json!({"id":"01A"}));
        assert_eq!(found["content"], "full content here");
        let missing = dispatch(
            &store,
            None,
            &json!({"jsonrpc":"2.0","id":9,"method":"tools/call",
                    "params":{"name":"get_clipboard_item","arguments":{"id":"nope"}}}),
        )
        .unwrap();
        assert_eq!(
            missing["result"]["content"][0]["text"].as_str().unwrap(),
            "null"
        );
    }

    #[test]
    fn exposure_scope_hides_clips_older_than_since() {
        let store = mem_store();
        seed(&store, "old", "text", "ancient note", 1_000_000_000_000);
        seed(&store, "new", "text", "fresh note", 1_700_000_000_000);
        let cutoff = Some(1_500_000_000_000);
        let listed: Value = call_since(&store, cutoff, "list_recent_clipboard", json!({}));
        let ids: Vec<&str> = listed
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["new"]);
        let searched: Value =
            call_since(&store, cutoff, "search_clipboard", json!({"query":"note"}));
        let ids: Vec<&str> = searched
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["new"]);
        let got = dispatch(
            &store,
            cutoff,
            &json!({"jsonrpc":"2.0","id":9,"method":"tools/call",
                    "params":{"name":"get_clipboard_item","arguments":{"id":"old"}}}),
        )
        .unwrap();
        assert_eq!(
            got["result"]["content"][0]["text"].as_str().unwrap(),
            "null"
        );
    }

    #[test]
    fn unknown_tool_is_error() {
        let store = mem_store();
        let resp = dispatch(
            &store,
            None,
            &json!({"jsonrpc":"2.0","id":9,"method":"tools/call",
                    "params":{"name":"delete_everything","arguments":{}}}),
        )
        .unwrap();
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn search_clipboard_without_query_is_invalid_params() {
        let store = mem_store();
        let resp = dispatch(
            &store,
            None,
            &json!({"jsonrpc":"2.0","id":9,"method":"tools/call",
                    "params":{"name":"search_clipboard","arguments":{}}}),
        )
        .unwrap();
        assert_eq!(resp["error"]["code"], -32602);
    }

    // ---- Fleet scope (B2) ----------------------------------------------------

    /// Seed one clip from THIS machine (TEST_SELF) and two from other machines.
    fn seed_fleet_fixture(store: &Store) {
        seed_with_source(
            store,
            "self1",
            TEST_SELF,
            "text",
            "note from this box",
            1_700_000_000_000,
        );
        seed_with_source(
            store,
            "otherA",
            "remote:other-host",
            "text",
            "note from box A",
            1_700_000_000_001,
        );
        seed_with_source(
            store,
            "otherB",
            "remote:third-host",
            "text",
            "note from box B",
            1_700_000_000_002,
        );
    }

    #[test]
    fn list_recent_fleet_excludes_self_source() {
        let store = mem_store();
        seed_fleet_fixture(&store);

        // scope:"fleet" => only the OTHER machines' rows.
        let fleet = call(&store, "list_recent_clipboard", json!({"scope":"fleet"}));
        let mut ids = ids_of(&fleet);
        ids.sort();
        assert_eq!(ids, ["otherA", "otherB"]);

        // default (no scope) => ALL rows incl. self.
        let all_default = call(&store, "list_recent_clipboard", json!({}));
        assert_eq!(ids_of(&all_default).len(), 3);

        // scope:"all" => ALL rows.
        let all = call(&store, "list_recent_clipboard", json!({"scope":"all"}));
        assert_eq!(ids_of(&all).len(), 3);

        // scope:"local" (deprecated alias for all) => ALL rows.
        let local = call(&store, "list_recent_clipboard", json!({"scope":"local"}));
        assert_eq!(ids_of(&local).len(), 3);

        // unknown scope falls back to all (defensive).
        let unknown = call(&store, "list_recent_clipboard", json!({"scope":"bogus"}));
        assert_eq!(ids_of(&unknown).len(), 3);
    }

    #[test]
    fn search_fleet_excludes_self_source() {
        let store = mem_store();
        seed_fleet_fixture(&store);

        // FTS path: "note" matches all three; fleet drops the self row.
        let fleet = call(
            &store,
            "search_clipboard",
            json!({"query":"note","scope":"fleet"}),
        );
        let mut ids = ids_of(&fleet);
        ids.sort();
        assert_eq!(ids, ["otherA", "otherB"]);

        let all_default = call(&store, "search_clipboard", json!({"query":"note"}));
        assert_eq!(ids_of(&all_default).len(), 3);

        let all = call(
            &store,
            "search_clipboard",
            json!({"query":"note","scope":"all"}),
        );
        assert_eq!(ids_of(&all).len(), 3);

        let local = call(
            &store,
            "search_clipboard",
            json!({"query":"note","scope":"local"}),
        );
        assert_eq!(ids_of(&local).len(), 3);

        let unknown = call(
            &store,
            "search_clipboard",
            json!({"query":"note","scope":"bogus"}),
        );
        assert_eq!(ids_of(&unknown).len(), 3);
    }

    #[test]
    fn search_fleet_empty_query_uses_list_path() {
        // An empty/whitespace query routes through list_clips, which must also
        // honor exclude_source for scope:"fleet".
        let store = mem_store();
        seed_fleet_fixture(&store);
        let fleet = call(
            &store,
            "search_clipboard",
            json!({"query":"   ","scope":"fleet"}),
        );
        let mut ids = ids_of(&fleet);
        ids.sort();
        assert_eq!(ids, ["otherA", "otherB"]);
    }

    #[test]
    fn fleet_scope_ands_with_since_ms() {
        let store = mem_store();
        // self (old + recent), other (old + recent).
        seed_with_source(
            &store,
            "self_old",
            TEST_SELF,
            "text",
            "old self",
            1_000_000_000_000,
        );
        seed_with_source(
            &store,
            "self_new",
            TEST_SELF,
            "text",
            "new self",
            1_700_000_000_000,
        );
        seed_with_source(
            &store,
            "other_old",
            "remote:other-host",
            "text",
            "old other",
            1_000_000_000_001,
        );
        seed_with_source(
            &store,
            "other_new",
            "remote:other-host",
            "text",
            "new other",
            1_700_000_000_001,
        );
        let cutoff = Some(1_500_000_000_000);

        // fleet AND since_ms => only the recent OTHER row.
        let listed = call_since(
            &store,
            cutoff,
            "list_recent_clipboard",
            json!({"scope":"fleet"}),
        );
        assert_eq!(ids_of(&listed), ["other_new"]);

        let searched = call_since(
            &store,
            cutoff,
            "search_clipboard",
            json!({"query":"other","scope":"fleet"}),
        );
        assert_eq!(ids_of(&searched), ["other_new"]);
    }

    #[test]
    fn get_clipboard_item_ignores_scope() {
        // get_clipboard_item has no scope; a self-origin id is still fetchable.
        let store = mem_store();
        seed_fleet_fixture(&store);
        let got = call(
            &store,
            "get_clipboard_item",
            json!({"id":"self1","scope":"fleet"}),
        );
        assert_eq!(got["id"], "self1");
    }

    // ---- tools_list schema (B1) ---------------------------------------------

    #[test]
    fn tools_list_advertises_scope_on_list_and_search_only() {
        let store = mem_store();
        let req = json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}});
        let resp = dispatch(&store, None, &req).expect("response");
        let tools = resp["result"]["tools"].as_array().unwrap();

        let by_name = |n: &str| {
            tools
                .iter()
                .find(|t| t["name"].as_str() == Some(n))
                .unwrap()
                .clone()
        };

        for tool_name in ["search_clipboard", "list_recent_clipboard"] {
            let t = by_name(tool_name);
            let scope = &t["inputSchema"]["properties"]["scope"];
            assert_eq!(scope["type"], "string", "{tool_name} scope type");
            let en: Vec<&str> = scope["enum"]
                .as_array()
                .unwrap_or_else(|| panic!("{tool_name} scope enum"))
                .iter()
                .map(|v| v.as_str().unwrap())
                .collect();
            assert_eq!(en, ["all", "fleet"], "{tool_name} scope enum values");
        }

        // get_clipboard_item must NOT advertise scope (by-id lookup).
        let get_tool = by_name("get_clipboard_item");
        assert!(
            get_tool["inputSchema"]["properties"]["scope"].is_null(),
            "get_clipboard_item must not advertise scope"
        );
    }

    // ---- Fleet backfill trigger decision (B3) -------------------------------

    fn fleet_call(scope: Option<&str>) -> Value {
        let args = match scope {
            Some(s) => json!({"query":"x","scope":s}),
            None => json!({"query":"x"}),
        };
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
               "params":{"name":"search_clipboard","arguments":args}})
    }

    #[test]
    fn backfill_disabled_when_flag_unset() {
        // flag_enabled=false => never triggers, regardless of scope.
        assert!(!fleet_backfill_should_trigger(
            false,
            false,
            &fleet_call(Some("fleet"))
        ));
    }

    #[test]
    fn backfill_triggers_only_on_first_fleet_call() {
        let msg = fleet_call(Some("fleet"));
        // First fleet call with the flag set and not-yet-done => trigger.
        assert!(fleet_backfill_should_trigger(true, false, &msg));
        // Once done, a second fleet call does NOT re-trigger (once-guard).
        assert!(!fleet_backfill_should_trigger(true, true, &msg));
    }

    #[test]
    fn backfill_not_triggered_by_non_fleet_calls() {
        // scope:"all" / "local" / absent / unknown never trigger.
        assert!(!fleet_backfill_should_trigger(
            true,
            false,
            &fleet_call(Some("all"))
        ));
        assert!(!fleet_backfill_should_trigger(
            true,
            false,
            &fleet_call(Some("local"))
        ));
        assert!(!fleet_backfill_should_trigger(
            true,
            false,
            &fleet_call(None)
        ));
        assert!(!fleet_backfill_should_trigger(
            true,
            false,
            &fleet_call(Some("bogus"))
        ));
        // initialize, ping, tools/list never trigger.
        let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
        let list = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}});
        let ping = json!({"jsonrpc":"2.0","id":1,"method":"ping","params":{}});
        assert!(!fleet_backfill_should_trigger(true, false, &init));
        assert!(!fleet_backfill_should_trigger(true, false, &list));
        assert!(!fleet_backfill_should_trigger(true, false, &ping));
    }

    #[test]
    fn maybe_fleet_backfill_flips_once_guard_only_when_eligible() {
        // The once-flag transition is deterministic without a live relay: we use
        // CINCH_MCP_FLEET unset so run_fleet_backfill is never reached, isolating
        // the decision/flag logic. (Env-coupled paths are covered by the pure
        // `fleet_backfill_should_trigger` tests above.)
        let store = mem_store();
        std::env::remove_var("CINCH_MCP_FLEET");
        let mut done = false;
        // Flag unset => never triggers, flag stays false.
        maybe_fleet_backfill(&store, TEST_SELF, &fleet_call(Some("fleet")), &mut done);
        assert!(
            !done,
            "flag must stay unset when CINCH_MCP_FLEET is not '1'"
        );
    }

    // ---- stdio envelope for a fleet call (B3) -------------------------------

    #[test]
    fn fleet_tools_call_produces_well_formed_envelope() {
        // A scope:"fleet" tools/call still produces a well-formed result envelope
        // whose content[0].text is a JSON array. (Backfill is gated by the env
        // flag and not exercised here; the local query path is what we assert.)
        let store = mem_store();
        seed_fleet_fixture(&store);
        let req = json!({"jsonrpc":"2.0","id":7,"method":"tools/call",
                         "params":{"name":"list_recent_clipboard","arguments":{"scope":"fleet"}}});
        let resp = dispatch(&store, None, &req).expect("response");
        assert_eq!(resp["jsonrpc"], "2.0");
        assert_eq!(resp["id"], 7);
        let text = resp["result"]["content"][0]["text"]
            .as_str()
            .expect("content text");
        let arr: Value = serde_json::from_str(text).expect("json array");
        assert!(arr.is_array());
        let mut ids = ids_of(&arr);
        ids.sort();
        assert_eq!(ids, ["otherA", "otherB"]);
    }
}
