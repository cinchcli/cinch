//! Newline-delimited JSON-RPC 2.0 router for the MCP stdio transport.

use crate::exit::ExitError;
use client_core::store::Store;
use serde_json::{json, Value};

const PROTOCOL_VERSION: &str = "2024-11-05";

pub fn serve_stdio(_store: &Store) -> Result<(), ExitError> {
    Ok(())
}

/// Handle one JSON-RPC message. Returns `Some(response)` for requests and
/// `None` for notifications (messages without an `id`, which must not be answered).
/// `since_ms` is the exposure-scope cutoff (None = full history); it bounds what
/// the tool calls return.
pub fn handle_request(store: &Store, since_ms: Option<i64>, msg: &Value) -> Option<Value> {
    let id = msg.get("id").cloned()?; // notifications carry no id
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "cinch", "version": env!("CARGO_PKG_VERSION") }
        })),
        "tools/list" => Ok(tools_list()),
        "tools/call" => handle_tool_call(store, since_ms, msg.get("params")),
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
            "description": "Full-text search the user's clipboard history. Returns matching clips as previews; use get_clipboard_item for full content.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Natural-language or keyword query." },
                    "limit": { "type": "integer", "description": "Max results (default 20, max 100)." }
                },
                "required": ["query"]
            }
        },
        {
            "name": "list_recent_clipboard",
            "description": "List the most recent clipboard items. limit=1 returns what was just copied.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "description": "Max results (default 20, max 100)." },
                    "source": { "type": "string", "description": "Optional source filter, e.g. a device id." }
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
use super::query::{clamp_limit, sanitize_fts_query};

fn handle_tool_call(
    store: &Store,
    since_ms: Option<i64>,
    params: Option<&Value>,
) -> Result<Value, (i64, String)> {
    let params = params.ok_or((-32602, "missing params".to_string()))?;
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    // Exposure-scope: keep only clips at or newer than the cutoff (None = all).
    let within = |created_at: i64| since_ms.map_or(true, |s| created_at >= s);

    let payload: Value = match name {
        "search_clipboard" => {
            let query = args.get("query").and_then(Value::as_str).unwrap_or("");
            let limit = clamp_limit(args.get("limit").and_then(Value::as_i64));
            let fts = sanitize_fts_query(query);
            // The empty-query path goes through list_clips (DB-filters by since_ms);
            // the FTS path uses search_clips (no since arg) so we filter in Rust.
            let rows = if fts.is_empty() {
                client_core::store::queries::list_clips(
                    store,
                    None,
                    Some(limit),
                    since_ms,
                    false,
                    limit,
                )
            } else {
                client_core::store::queries::search_clips(store, &fts, limit)
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
            let rows = client_core::store::queries::list_clips(
                store,
                source,
                Some(limit),
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

    fn mem_store() -> Store {
        Store::open(Path::new(":memory:")).expect("open in-memory store")
    }

    #[test]
    fn initialize_reports_server_info() {
        let store = mem_store();
        let req = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}});
        let resp = handle_request(&store, None, &req).expect("response");
        assert_eq!(resp["result"]["serverInfo"]["name"], "cinch");
        assert_eq!(resp["result"]["protocolVersion"], PROTOCOL_VERSION);
    }

    #[test]
    fn tools_list_has_three_read_tools() {
        let store = mem_store();
        let req = json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}});
        let resp = handle_request(&store, None, &req).expect("response");
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
        assert!(handle_request(&store, None, &req).is_none());
    }

    #[test]
    fn unknown_method_is_jsonrpc_error() {
        let store = mem_store();
        let req = json!({"jsonrpc":"2.0","id":3,"method":"bogus"});
        let resp = handle_request(&store, None, &req).expect("response");
        assert_eq!(resp["error"]["code"], -32601);
    }

    use client_core::store::models::StoredClip;
    use client_core::store::queries::insert_clip;

    fn seed(store: &Store, id: &str, content_type: &str, content: &str, created_at: i64) {
        insert_clip(
            store,
            &StoredClip {
                id: id.to_string(),
                source: "remote:macbook".to_string(),
                source_key: None,
                content_type: content_type.to_string(),
                content: Some(content.as_bytes().to_vec()),
                media_path: None,
                byte_size: content.len() as i64,
                created_at,
                pinned: false,
                pinned_at: None,
                synced: true,
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
        let resp = handle_request(store, since_ms, &req).expect("response");
        // Tool result envelope: content[0].text is a JSON string of the clip array.
        let text = resp["result"]["content"][0]["text"].as_str().expect("text");
        serde_json::from_str(text).expect("json array")
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
        let missing = handle_request(
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
        let got = handle_request(
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
        let resp = handle_request(
            &store,
            None,
            &json!({"jsonrpc":"2.0","id":9,"method":"tools/call",
                    "params":{"name":"delete_everything","arguments":{}}}),
        )
        .unwrap();
        assert_eq!(resp["error"]["code"], -32601);
    }
}
