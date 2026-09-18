//! The MCP client: the handshake, the tool list, and a tool call.
//!
//! Deliberately thin. Everything that makes an MCP tool governable — the
//! namespace, the precedence, the policy check, the audit — is done by
//! [`register`](crate::register) against `orrery-tools`, because an MCP tool is
//! an ordinary [`ToolRef`](orrery_proto::ToolRef) and not a special case. This
//! module's whole job is to speak the protocol.

use std::sync::Arc;

use orrery_jsonrpc::{Peer, RpcError};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::error::McpError;

/// The protocol revision this client asks for.
///
/// Open question 3, decided: **pin it, and accept a short list.** The spec
/// moves, so the client names one revision in `initialize`, accepts any in
/// [`SUPPORTED`], and refuses anything else by name rather than carrying on and
/// discovering the difference in production.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Every revision this client will talk, newest first.
///
/// A server that answers with one of these is spoken to; anything else is
/// [`McpError::ProtocolVersion`]. Adding a revision here is a deliberate act
/// with a test behind it, which is the drift check plan 08 task 2 asks for,
/// spelled as a constant rather than a generator: MCP has no schema artefact in
/// this tree to diff against.
pub const SUPPORTED: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// `initialize`.
pub const INITIALIZE: &str = "initialize";
/// The notification that follows a successful `initialize`.
pub const INITIALIZED: &str = "notifications/initialized";
/// `tools/list`.
pub const TOOLS_LIST: &str = "tools/list";
/// `tools/call`.
pub const TOOLS_CALL: &str = "tools/call";
/// The notification a server sends when its tool set moves.
pub const TOOLS_LIST_CHANGED: &str = "notifications/tools/list_changed";

/// One tool, as the server describes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct McpTool {
    /// Its name inside the server. The short name; the namespace is ours.
    pub name: String,
    /// What it does, for the model.
    #[serde(default)]
    pub description: String,
    /// The JSON Schema for its input, as the server wrote it.
    #[serde(rename = "inputSchema", default = "empty_object")]
    pub input_schema: Value,
}

fn empty_object() -> Value {
    serde_json::json!({ "type": "object" })
}

/// What `initialize` came back with.
#[derive(Clone, Debug, PartialEq)]
pub struct InitializeResult {
    /// The revision the server settled on.
    pub protocol_version: String,
    /// What it calls itself.
    pub server_name: String,
    /// Its version.
    pub server_version: String,
    /// Whether it will tell us when its tool set moves.
    pub supports_tool_list_changed: bool,
    /// Everything it said, unread keys included.
    pub raw: Value,
}

/// What a `tools/call` came back with.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolResult {
    /// The content blocks, as sent.
    pub content: Vec<Value>,
    /// Whether the server is reporting a failure rather than an answer.
    pub is_error: bool,
    /// Everything it said.
    pub raw: Value,
}

impl ToolResult {
    /// Every text block, joined. The usual thing a caller wants.
    #[must_use]
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter(|c| c.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|c| c.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("")
    }
}

/// One connected MCP server.
///
/// Cheap to clone the peer behind it, so a tool call and a health check can be
/// issued from different tasks over the same connection.
pub struct McpClient {
    peer: Peer,
    server: String,
    negotiated: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for McpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpClient")
            .field("server", &self.server)
            .field("connected", &self.peer.is_connected())
            .finish_non_exhaustive()
    }
}

impl McpClient {
    /// A client over an established connection.
    #[must_use]
    pub fn over(peer: Peer, server: impl Into<String>) -> Self {
        Self {
            peer,
            server: server.into(),
            negotiated: Arc::new(Mutex::new(None)),
        }
    }

    /// The server's name, without the `mcp.` prefix.
    #[must_use]
    pub fn server(&self) -> &str {
        &self.server
    }

    /// Whether the connection is still there.
    ///
    /// False the moment the far side goes away, not when the last handle is
    /// dropped — which is what lets [`health`](crate::health) degrade a dead
    /// server's tools instead of a turn waiting out its budget.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.peer.is_connected()
    }

    /// The revision `initialize` settled on, once it has run.
    #[must_use]
    pub fn negotiated_version(&self) -> Option<String> {
        self.negotiated.lock().clone()
    }

    /// The underlying peer, for a caller that needs to send something this
    /// module has not been taught.
    #[must_use]
    pub fn peer(&self) -> &Peer {
        &self.peer
    }

    /// The handshake: `initialize`, then `notifications/initialized`.
    ///
    /// # Errors
    ///
    /// [`McpError::ProtocolVersion`] when the server answers with a revision
    /// outside [`SUPPORTED`], and [`McpError::Rpc`] or [`McpError::Malformed`]
    /// when the call itself fails.
    pub async fn initialize(&self) -> Result<InitializeResult, McpError> {
        let raw = self
            .call(
                INITIALIZE,
                serde_json::json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "clientInfo": { "name": "orrery", "version": env!("CARGO_PKG_VERSION") },
                }),
            )
            .await?;

        let theirs = raw
            .get("protocolVersion")
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::malformed(&self.server, INITIALIZE, "no `protocolVersion`"))?
            .to_owned();
        if !SUPPORTED.contains(&theirs.as_str()) {
            return Err(McpError::ProtocolVersion {
                server: self.server.clone(),
                theirs,
                ours: SUPPORTED.to_vec(),
            });
        }

        let info = raw.get("serverInfo");
        let result = InitializeResult {
            supports_tool_list_changed: raw
                .pointer("/capabilities/tools/listChanged")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            server_name: info
                .and_then(|i| i.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            server_version: info
                .and_then(|i| i.get("version"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            protocol_version: theirs.clone(),
            raw,
        };
        *self.negotiated.lock() = Some(theirs);

        // The spec says the client confirms. A server that waits for it and
        // never gets it answers nothing afterwards.
        self.peer.notify(INITIALIZED, serde_json::json!({}));
        Ok(result)
    }

    /// The server's tools, as it describes them.
    ///
    /// # Errors
    ///
    /// [`McpError::Rpc`] or [`McpError::Malformed`].
    pub async fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
        let raw = self.call(TOOLS_LIST, serde_json::json!({})).await?;
        let tools = raw
            .get("tools")
            .cloned()
            .ok_or_else(|| McpError::malformed(&self.server, TOOLS_LIST, "no `tools` array"))?;
        serde_json::from_value(tools)
            .map_err(|e| McpError::malformed(&self.server, TOOLS_LIST, e.to_string()))
    }

    /// Call one of its tools.
    ///
    /// # Errors
    ///
    /// [`McpError::Rpc`] when the server refuses, [`McpError::Malformed`] when
    /// it answers with something the spec does not describe.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<ToolResult, McpError> {
        let raw = self
            .call(
                TOOLS_CALL,
                serde_json::json!({ "name": name, "arguments": arguments }),
            )
            .await?;
        Ok(ToolResult {
            content: raw
                .get("content")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            is_error: raw.get("isError").and_then(Value::as_bool).unwrap_or(false),
            raw,
        })
    }

    async fn call(&self, method: &str, params: Value) -> Result<Value, McpError> {
        self.peer
            .call(method, params, &CancellationToken::new())
            .await
            .map_err(|e: RpcError| McpError::rpc(&self.server, e))
    }
}

#[cfg(test)]
mod tests {
    use super::{PROTOCOL_VERSION, SUPPORTED, ToolResult};

    #[test]
    fn the_pinned_version_is_one_we_accept() {
        assert!(SUPPORTED.contains(&PROTOCOL_VERSION));
        assert_eq!(SUPPORTED[0], PROTOCOL_VERSION, "newest first");
    }

    #[test]
    fn text_joins_only_the_text_blocks() {
        let result = ToolResult {
            content: vec![
                serde_json::json!({ "type": "text", "text": "a" }),
                serde_json::json!({ "type": "image", "data": "…" }),
                serde_json::json!({ "type": "text", "text": "b" }),
            ],
            is_error: false,
            raw: serde_json::Value::Null,
        };
        assert_eq!(result.text(), "ab");
    }
}
