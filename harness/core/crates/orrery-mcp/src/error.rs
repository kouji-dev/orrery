//! What can go wrong reaching an MCP server, and what deliberately cannot.
//!
//! There is **no `Denied` variant**. A refused MCP tool call is an
//! `Outcome::Denied` from [`Registry::dispatch`](orrery_tools::Registry::dispatch),
//! exactly like a refused local one — an MCP server is behind the same policy
//! as everything else, and that is only true if the refusal takes the same
//! shape. What is in here is the connection failing.

use orrery_jsonrpc::RpcError;

/// A call to, or a connection with, an MCP server that did not work.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum McpError {
    /// The server refused, in JSON-RPC's own vocabulary.
    #[error("mcp.{server}: {source}")]
    Rpc {
        /// Which server.
        server: String,
        /// What it said.
        #[source]
        source: RpcError,
    },
    /// The server speaks a protocol revision this client does not.
    ///
    /// Refused rather than guessed: an MCP client that carries on against an
    /// unknown revision is a client that will one day silently drop a field.
    #[error(
        "mcp.{server} speaks protocol `{theirs}`; this client speaks {ours:?} — \
         pin the server to one of those, or update the pinned list"
    )]
    ProtocolVersion {
        /// Which server.
        server: String,
        /// What it offered.
        theirs: String,
        /// What we accept.
        ours: Vec<&'static str>,
    },
    /// The server answered something that is not the shape the spec says.
    #[error("mcp.{server} answered `{method}` with something unusable: {message}")]
    Malformed {
        /// Which server.
        server: String,
        /// Which method.
        method: String,
        /// What was wrong with it.
        message: String,
    },
    /// The transport could not be established.
    #[error("mcp.{server} could not be reached: {message}")]
    Transport {
        /// Which server.
        server: String,
        /// What went wrong.
        message: String,
    },
    /// The server's name is not a legal extension id.
    #[error("`{name}` is not a usable MCP server name: {message}")]
    BadName {
        /// The offending name.
        name: String,
        /// Why.
        message: String,
    },
    /// Nothing is discovered under that name.
    #[error("no MCP server called `{server}` was discovered")]
    Unknown {
        /// The name that was asked for.
        server: String,
    },
}

impl McpError {
    /// An RPC failure against a named server.
    #[must_use]
    pub fn rpc(server: impl Into<String>, source: RpcError) -> Self {
        McpError::Rpc {
            server: server.into(),
            source,
        }
    }

    /// An answer that is not the shape the spec describes.
    #[must_use]
    pub fn malformed(
        server: impl Into<String>,
        method: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        McpError::Malformed {
            server: server.into(),
            method: method.into(),
            message: message.into(),
        }
    }
}
