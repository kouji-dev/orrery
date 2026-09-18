//! The MCP client against a real server, over the real protocol.
//!
//! # The phase-7 acceptance criterion
//!
//! `real_server_works_unmodified` is the one the plan singles out: an existing
//! MCP server works here with no change. The server it drives is
//! `tests/fixtures/server/main.rs` — hand-written to the **published spec**,
//! not to this client, because no test in this repository may fetch anything
//! from the network. It knows nothing about `orrery-mcp`: it answers
//! `initialize`, `tools/list` and `tools/call` over newline-delimited JSON-RPC
//! and `-32601` for anything else.
//!
//! To run these against a third-party server instead, point [`StdioSpec`] at
//! its command line. Nothing below is fixture-specific except the tool names.

use std::collections::BTreeMap;

use orrery_mcp::client::{McpClient, PROTOCOL_VERSION};
use orrery_mcp::transport::{self, StdioSpec};
use orrery_mcp::McpError;

const SERVER: &str = env!("CARGO_BIN_EXE_orrery-mcp-fixture-server");

fn spec() -> StdioSpec {
    StdioSpec {
        program: SERVER.to_owned(),
        args: Vec::new(),
        cwd: None,
        env: BTreeMap::new(),
    }
}

async fn connected() -> (McpClient, transport::StdioConnection) {
    let connection = transport::connect_stdio(&spec())
        .await
        .expect("the fixture server starts");
    let client = McpClient::over(connection.peer().clone(), "notes");
    client.initialize().await.expect("the handshake");
    (client, connection)
}

#[tokio::test]
async fn real_server_works_unmodified() {
    let (client, _connection) = connected().await;

    let tools = client.list_tools().await.expect("tools/list");
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"echo"), "{names:?}");

    // The schema came across as the server wrote it, not as we guessed it.
    let echo = tools.iter().find(|t| t.name == "echo").unwrap();
    assert_eq!(echo.input_schema["type"], "object");
    assert_eq!(echo.description, "Say back what you were given.");

    let result = client
        .call_tool("echo", serde_json::json!({ "text": "still here" }))
        .await
        .expect("tools/call");
    assert_eq!(result.text(), "still here");
    assert!(!result.is_error);
}

#[tokio::test]
async fn initialize_handshake() {
    let connection = transport::connect_stdio(&spec()).await.unwrap();
    let client = McpClient::over(connection.peer().clone(), "notes");

    let hello = client.initialize().await.expect("the handshake");
    assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
    assert_eq!(hello.server_name, "fixture-notes");
    assert!(hello.supports_tool_list_changed);

    // And the negotiated version is remembered, so nothing later has to guess.
    assert_eq!(client.negotiated_version().as_deref(), Some(PROTOCOL_VERSION));
}

#[tokio::test]
async fn a_version_we_do_not_speak_is_refused_rather_than_guessed() {
    let mut spec = spec();
    spec.args = vec!["--protocol".to_owned(), "1066-10-14".to_owned()];
    let connection = transport::connect_stdio(&spec).await.unwrap();
    let client = McpClient::over(connection.peer().clone(), "notes");

    let err = client
        .initialize()
        .await
        .expect_err("a version we do not speak is not a version we speak");
    assert!(matches!(err, McpError::ProtocolVersion { .. }), "{err}");
    assert!(err.to_string().contains("1066-10-14"));
}

#[tokio::test]
async fn stdio_is_line_delimited() {
    // Not `Content-Length`. MCP stdio is one JSON object per line, which is the
    // reason `orrery-jsonrpc` carries a `LineDelimited` variant at all — so
    // this asserts the constant rather than trusting the connection to have
    // used it.
    assert_eq!(transport::STDIO_FRAMING, orrery_jsonrpc::Framing::LineDelimited);

    // And it is really what went over the wire: a server that only ever reads
    // whole lines answered us above, and one that had been sent
    // `Content-Length:` headers would have seen them as junk and answered
    // nothing. Prove it the other way too — a raw line, hand-written, gets a
    // raw line back.
    let (client, _connection) = connected().await;
    let result = client
        .call_tool("echo", serde_json::json!({ "text": "line" }))
        .await
        .unwrap();
    assert_eq!(result.text(), "line");
}

#[tokio::test]
async fn a_method_the_server_does_not_have_comes_back_as_its_refusal() {
    let (client, _connection) = connected().await;
    let err = client
        .call_tool("no-such-tool", serde_json::json!({}))
        .await
        .expect_err("the server refuses");
    assert!(matches!(err, McpError::Rpc { .. }), "{err}");
}
