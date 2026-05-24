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

// Implemented in Task 5.
fn handle_tool_call(
    _store: &Store,
    _since_ms: Option<i64>,
    _params: Option<&Value>,
) -> Result<Value, (i64, String)> {
    Err((-32601, "tools/call not yet implemented".to_string()))
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
}
