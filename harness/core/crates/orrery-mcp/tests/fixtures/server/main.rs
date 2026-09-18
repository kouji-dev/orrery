//! A conformant MCP server over stdio, as a committed test fixture.
//!
//! # Why this is hand-written
//!
//! The plan asks for "an actual off-the-shelf MCP server, vendored as a test
//! fixture". Vendoring one means fetching it, and nothing in this repository's
//! test suite may reach the network — so this is the closest correct thing: a
//! server written to the **published protocol**, not to this client. It speaks
//! `initialize` / `notifications/initialized` / `tools/list` / `tools/call`
//! over newline-delimited JSON-RPC 2.0 on stdin and stdout, answers `-32601`
//! for a method it does not know, and emits `notifications/tools/list_changed`
//! when its tool set changes. Nothing in here knows what `orrery-mcp` is.
//!
//! Open question 1 is decided in the plan file: this fixture, pinned, plus a
//! conformance note. A third-party server can be run against the same tests by
//! pointing `StdioSpec` at it, and `tests/client.rs` says so.
//!
//! Behaviours a test can ask for:
//!
//! - `--protocol <version>` — answer `initialize` with that version instead of
//!   the pinned one, so version negotiation can be exercised.
//! - the `grow` tool — adds a second tool and notifies `tools/list_changed`.
//! - the `die` tool — exits without answering, so a client can watch a server
//!   disappear mid-session.

use std::io::{BufRead, Write};

use serde_json::{Value, json};

/// The protocol revision this fixture implements.
const PROTOCOL: &str = "2025-06-18";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let protocol = args
        .iter()
        .position(|a| a == "--protocol")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| PROTOCOL.to_owned());

    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    let mut grown = false;

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(frame): Result<Value, _> = serde_json::from_str(line) else {
            continue;
        };
        let method = frame.get("method").and_then(Value::as_str).unwrap_or("");
        let id = frame.get("id").cloned();

        // A notification has no id and takes no answer.
        let Some(id) = id else {
            continue;
        };

        let answer = match method {
            "initialize" => Ok(json!({
                "protocolVersion": protocol,
                "capabilities": { "tools": { "listChanged": true } },
                "serverInfo": { "name": "fixture-notes", "version": "0.1.0" },
            })),
            "tools/list" => Ok(json!({ "tools": tools(grown) })),
            "tools/call" => {
                let params = frame.get("params").cloned().unwrap_or(json!({}));
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
                match name {
                    "echo" => {
                        let text = arguments
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned();
                        Ok(json!({
                            "content": [{ "type": "text", "text": text }],
                            "isError": false,
                        }))
                    }
                    "grow" => {
                        grown = true;
                        // Answer first, then tell the client its tool set moved.
                        send(
                            &mut out,
                            &json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": { "content": [{ "type": "text", "text": "grown" }], "isError": false },
                            }),
                        );
                        send(
                            &mut out,
                            &json!({
                                "jsonrpc": "2.0",
                                "method": "notifications/tools/list_changed",
                                "params": {},
                            }),
                        );
                        continue;
                    }
                    "reverse" if grown => {
                        let text: String = arguments
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .chars()
                            .rev()
                            .collect();
                        Ok(json!({
                            "content": [{ "type": "text", "text": text }],
                            "isError": false,
                        }))
                    }
                    // Leave without answering. The client must notice.
                    "die" => std::process::exit(0),
                    other => Err((-32602, format!("no such tool: {other}"))),
                }
            }
            other => Err((-32601, format!("method not found: {other}"))),
        };

        let frame = match answer {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err((code, message)) => {
                json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
            }
        };
        send(&mut out, &frame);
    }
}

fn tools(grown: bool) -> Vec<Value> {
    let mut tools = vec![
        json!({
            "name": "echo",
            "description": "Say back what you were given.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"],
            },
        }),
        json!({
            "name": "grow",
            "description": "Add a tool to this server's list, mid-session.",
            "inputSchema": { "type": "object" },
        }),
        json!({
            "name": "die",
            "description": "Exit without answering.",
            "inputSchema": { "type": "object" },
        }),
    ];
    if grown {
        tools.push(json!({
            "name": "reverse",
            "description": "Say back what you were given, backwards.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"],
            },
        }));
    }
    tools
}

/// One JSON object, one line, flushed. That is the whole of MCP stdio framing.
fn send(out: &mut std::io::Stdout, frame: &Value) {
    let _ = writeln!(out, "{frame}");
    let _ = out.flush();
}
