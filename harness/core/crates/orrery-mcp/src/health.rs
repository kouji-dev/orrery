//! Discovery, connection, and what happens when a server dies.
//!
//! # Discovery and connection are different things
//!
//! A server is **discovered** at session start, always: it is in the manifest,
//! the load ledger names it, and `orrery mcp list` can show it — with **no
//! process started**. It is **connected** the first time one of its tools is
//! actually called. `health::discovered_but_not_connected` asserts the absence
//! of the process, not the presence of a flag.
//!
//! # A dead server degrades its tools; it does not stall a turn
//!
//! When the far side goes away, [`Peer::is_connected`](orrery_jsonrpc::Peer)
//! goes false the moment the read loop ends — not when the last handle is
//! dropped — so a call to a dead server comes back
//! [`Outcome::Unloaded`](orrery_proto::Outcome) **immediately** instead of
//! waiting out its wall-clock budget. The model is told the tool is not there
//! and the turn carries on. A reconnect is one more connect: the next call
//! starts a fresh process and the tools work again.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use orrery_jsonrpc::{Handler, NoHandler};
use orrery_proto::{Layer, Outcome, ToolRef};
use orrery_tools::{CallCtx, ToolError, ToolHost};
use parking_lot::Mutex;
use serde_json::Value;

use crate::client::McpClient;
use crate::error::McpError;
use crate::transport::{StdioConnection, TransportSpec, connect_stdio_with};

/// Where a server is in its lifecycle.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Health {
    /// In the manifest. **No process has been started.**
    Discovered,
    /// Connected and answering.
    Connected,
    /// It was connected and is not any more. Its tools degrade; they do not
    /// stall.
    Failed {
        /// What happened, as far as we can tell.
        reason: String,
    },
}

/// One server as configuration declares it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerSpec {
    /// Its name, without the `mcp.` prefix.
    pub name: String,
    /// How to reach it.
    pub transport: TransportSpec,
    /// Which config layer declared it.
    pub layer: Layer,
}

impl ServerSpec {
    /// A stdio server declared at a layer.
    #[must_use]
    pub fn stdio(name: impl Into<String>, spec: crate::transport::StdioSpec, layer: Layer) -> Self {
        Self {
            name: name.into(),
            transport: TransportSpec::Stdio(spec),
            layer,
        }
    }
}

/// A live connection to one server.
struct Session {
    client: Arc<McpClient>,
    /// Held so the child is killed when the session is dropped, which is the
    /// whole of its job — there is nothing to read from it.
    #[allow(dead_code, reason = "held for its Drop: dropping it kills the child")]
    connection: StdioConnection,
}

/// Every discovered server, and the ones that have been connected.
///
/// Discovery fills this; nothing in [`Servers::discover`] starts a process.
pub struct Servers {
    specs: Vec<ServerSpec>,
    sessions: Mutex<BTreeMap<String, Session>>,
    /// Whichever servers have died since they were connected.
    failed: Mutex<BTreeMap<String, String>>,
    /// How many processes this has ever started. The `discovered_but_not
    /// _connected` assertion is a count, not a flag.
    started: AtomicUsize,
    handler: Arc<dyn Handler>,
}

impl std::fmt::Debug for Servers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Servers")
            .field("discovered", &self.specs.len())
            .field("connected", &self.sessions.lock().len())
            .field("started", &self.started.load(Ordering::SeqCst))
            .finish_non_exhaustive()
    }
}

impl Servers {
    /// Take the manifest's servers. **Starts nothing.**
    #[must_use]
    pub fn discover(specs: Vec<ServerSpec>) -> Self {
        Self {
            specs,
            sessions: Mutex::new(BTreeMap::new()),
            failed: Mutex::new(BTreeMap::new()),
            started: AtomicUsize::new(0),
            handler: Arc::new(NoHandler),
        }
    }

    /// Answer what a server says unprompted — `tools/list_changed`, above all.
    #[must_use]
    pub fn with_handler(mut self, handler: Arc<dyn Handler>) -> Self {
        self.handler = handler;
        self
    }

    /// Every discovered server, whether or not it has been connected.
    #[must_use]
    pub fn discovered(&self) -> &[ServerSpec] {
        &self.specs
    }

    /// One server's declaration.
    #[must_use]
    pub fn spec(&self, name: &str) -> Option<&ServerSpec> {
        self.specs.iter().find(|s| s.name == name)
    }

    /// How many processes have been started, ever.
    ///
    /// Zero after discovery, and that is the assertion.
    #[must_use]
    pub fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }

    /// Where a server is in its lifecycle, right now.
    #[must_use]
    pub fn health(&self, name: &str) -> Option<Health> {
        self.spec(name)?;
        if let Some(session) = self.sessions.lock().get(name) {
            if session.client.is_connected() {
                return Some(Health::Connected);
            }
            return Some(Health::Failed {
                reason: "the connection closed".to_owned(),
            });
        }
        Some(match self.failed.lock().get(name) {
            Some(reason) => Health::Failed {
                reason: reason.clone(),
            },
            None => Health::Discovered,
        })
    }

    /// The client for a server, connecting it if this is the first time it is
    /// needed.
    ///
    /// A server whose connection has since died is reconnected here: the dead
    /// session is dropped — which kills whatever is left of the process — and a
    /// fresh one takes its place.
    ///
    /// # Errors
    ///
    /// [`McpError::Unknown`] for a name that was never discovered,
    /// [`McpError::Transport`] when it cannot be started, and whatever
    /// [`McpClient::initialize`] raises.
    pub async fn connect(&self, name: &str) -> Result<Arc<McpClient>, McpError> {
        if let Some(session) = self.sessions.lock().get(name) {
            if session.client.is_connected() {
                return Ok(session.client.clone());
            }
        }
        // Dead, or never connected. Drop whatever is there and start again.
        if let Some(gone) = self.sessions.lock().remove(name) {
            drop(gone);
            self.failed.lock().insert(
                name.to_owned(),
                "the connection closed; reconnecting".to_owned(),
            );
        }

        let spec = self.spec(name).ok_or_else(|| McpError::Unknown {
            server: name.to_owned(),
        })?;
        let TransportSpec::Stdio(stdio) = &spec.transport else {
            return Err(McpError::Transport {
                server: name.to_owned(),
                message: "only the stdio transport is connected from here; \
                          HTTP is one request at a time through `Broker::net`"
                    .to_owned(),
            });
        };

        self.started.fetch_add(1, Ordering::SeqCst);
        let connection = connect_stdio_with(stdio, self.handler.clone()).await?;
        let client = Arc::new(McpClient::over(connection.peer().clone(), name));
        client.initialize().await?;

        self.failed.lock().remove(name);
        self.sessions.lock().insert(
            name.to_owned(),
            Session {
                client: client.clone(),
                connection,
            },
        );
        Ok(client)
    }

    /// Note that a server has gone, without waiting to find out.
    pub fn mark_failed(&self, name: &str, reason: impl Into<String>) {
        let reason = reason.into();
        tracing::warn!(target: "orrery.mcp.health", server = name, %reason, "MCP server degraded");
        self.sessions.lock().remove(name);
        self.failed.lock().insert(name.to_owned(), reason);
    }

    /// Stop a server and forget its session. It stays discovered.
    pub fn disconnect(&self, name: &str) {
        self.sessions.lock().remove(name);
    }
}

/// The [`ToolHost`] that carries an `mcp.<server>` call to its server.
///
/// Registered as the registry's host, so an MCP tool is dispatched by exactly
/// the same seven steps as a local one — the policy check included, because
/// there is no other way in.
#[derive(Clone)]
pub struct McpHost {
    servers: Arc<Servers>,
}

impl std::fmt::Debug for McpHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpHost")
            .field("servers", &self.servers)
            .finish()
    }
}

impl McpHost {
    /// A host over a set of discovered servers.
    #[must_use]
    pub fn new(servers: Arc<Servers>) -> Self {
        Self { servers }
    }

    /// The servers behind it.
    #[must_use]
    pub fn servers(&self) -> &Arc<Servers> {
        &self.servers
    }
}

/// The server name inside an `mcp.<server>` extension id.
#[must_use]
pub fn server_of(r#ref: &ToolRef) -> Option<&str> {
    r#ref.ext.as_str().strip_prefix(crate::register::PREFIX)
}

#[async_trait]
impl ToolHost for McpHost {
    /// Carry the call, or say the tool is not there. **Never stall.**
    async fn call(
        &self,
        r#ref: &ToolRef,
        input: Value,
        _ctx: &CallCtx,
    ) -> Result<Outcome, ToolError> {
        let Some(server) = server_of(r#ref) else {
            return Ok(Outcome::Unloaded {
                ext: r#ref.ext.clone(),
            });
        };

        let client = match self.servers.connect(server).await {
            Ok(client) => client,
            Err(e) => {
                // A server that will not start is a degraded tool, not a
                // failed turn: the model is told, and the turn carries on.
                self.servers.mark_failed(server, e.to_string());
                return Ok(Outcome::Unloaded {
                    ext: r#ref.ext.clone(),
                });
            }
        };

        match client.call_tool(&r#ref.name, input).await {
            Ok(result) if result.is_error => Ok(Outcome::Failed {
                code: "mcp-tool-error".to_owned(),
                message: result.text(),
            }),
            Ok(result) => Ok(Outcome::Ok {
                surface: None,
                value: Some(result.raw),
            }),
            Err(McpError::Rpc {
                source: orrery_jsonrpc::RpcError::Closed,
                ..
            }) => {
                self.servers
                    .mark_failed(server, "the connection closed mid-call");
                Ok(Outcome::Unloaded {
                    ext: r#ref.ext.clone(),
                })
            }
            Err(e) => Ok(Outcome::Failed {
                code: "mcp-transport".to_owned(),
                message: e.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::server_of;
    use orrery_proto::ToolRef;

    #[test]
    fn the_server_name_comes_back_out_of_the_reference() {
        let r#ref: ToolRef = "mcp.jira.create_issue".parse().unwrap();
        assert_eq!(server_of(&r#ref), Some("jira"));

        let local: ToolRef = "ripgrep.search".parse().unwrap();
        assert_eq!(server_of(&local), None);
    }
}
