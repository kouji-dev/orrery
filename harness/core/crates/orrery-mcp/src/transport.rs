//! The two transports, both on `orrery-jsonrpc`.
//!
//! # stdio is line-delimited, and that is why the variant exists
//!
//! MCP over stdio is **one JSON object per line** — not `Content-Length`
//! framing. [`Framing::LineDelimited`] was added to `orrery-jsonrpc` for exactly
//! this, and [`STDIO_FRAMING`] is the single place that choice is made, so no
//! second call site can quietly disagree with it.
//!
//! # Where the brokering is, and where it honestly is not
//!
//! HTTP goes through `Broker::net`, which is the brokered path in full: a
//! capability token per request, redeemed where the request is made, and the
//! ceiling applied while the bytes arrive.
//!
//! stdio does **not** go through `Broker::spawn`, and that is a gap rather than
//! a decision. `orrery_broker::proc::Child` gives the child `Stdio::null()` for
//! stdin and consumes itself in `wait()`; it is built for a tool that runs, is
//! bounded and finishes, not for a session that talks both ways for the length
//! of a turn. Creating an interactive child needs a `spawn_interactive` on the
//! broker — a handle with a stdin, a stdout and a kill grip — which belongs in
//! `orrery-broker` and is not this crate's to add. Until then
//! [`connect_stdio`] creates the process directly, with
//! [`kill_on_drop`](tokio::process::Command::kill_on_drop) so nothing outlives
//! its [`StdioConnection`], and the policy check that decides *whether* a
//! server may be connected at all lives in [`health`](crate::health).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use orrery_broker::{Broker, NetRequest, NetResponse};
use orrery_jsonrpc::framing::{Framing, StderrRing};
use orrery_jsonrpc::{Handler, NoHandler, Peer};
use orrery_policy::CapabilityToken;
use orrery_tools::ToolBudget;
use serde_json::Value;

use crate::error::McpError;

/// How MCP frames messages on stdio. One JSON object per line.
pub const STDIO_FRAMING: Framing = Framing::LineDelimited;

/// How much of a server's stderr to keep. The **end** of it, which is where the
/// reason it died lives.
pub const STDERR_TAIL: usize = 16 * 1024;

/// How to start a server over stdio.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StdioSpec {
    /// The program.
    pub program: String,
    /// Its arguments, already split. Nothing here ever reaches a shell.
    pub args: Vec<String>,
    /// Where to run it.
    pub cwd: Option<PathBuf>,
    /// Environment overrides. Empty leaves the host's environment alone.
    pub env: BTreeMap<String, String>,
}

impl StdioSpec {
    /// A program with no arguments.
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            ..Self::default()
        }
    }

    /// Add an argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// The command text a `spawn(...)` rule would match.
    #[must_use]
    pub fn command_text(&self) -> String {
        std::iter::once(self.program.clone())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// Where a server lives.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransportSpec {
    /// A child process speaking newline-delimited JSON-RPC on its pipes.
    Stdio(StdioSpec),
    /// A streamable HTTP endpoint, reached through the broker's `net`.
    Http(HttpSpec),
}

/// A streamable HTTP endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpSpec {
    /// Where to POST.
    pub url: String,
    /// Headers to send with every request.
    pub headers: BTreeMap<String, String>,
}

impl HttpSpec {
    /// An endpoint with no extra headers.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            headers: BTreeMap::new(),
        }
    }
}

/// A running stdio server and the connection to it.
///
/// Dropping it kills the child. That is the whole lifetime story: there is no
/// way to hold a peer whose process has been forgotten.
#[derive(Debug)]
pub struct StdioConnection {
    peer: Peer,
    child: tokio::process::Child,
    stderr: Arc<StderrRing>,
}

impl StdioConnection {
    /// The connection.
    #[must_use]
    pub fn peer(&self) -> &Peer {
        &self.peer
    }

    /// The child's process id, while it has one.
    #[must_use]
    pub fn process_id(&self) -> Option<u32> {
        self.child.id()
    }

    /// The tail of the server's stderr, which is where it says why it died.
    #[must_use]
    pub fn stderr_tail(&self) -> String {
        self.stderr.tail()
    }

    /// Stop it now.
    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }

    /// Wait for it to leave on its own.
    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }
}

/// Start a server over stdio and speak JSON-RPC to it.
///
/// # Errors
///
/// [`McpError::Transport`] when the process cannot be started or its pipes
/// cannot be taken.
pub async fn connect_stdio(spec: &StdioSpec) -> Result<StdioConnection, McpError> {
    connect_stdio_with(spec, Arc::new(NoHandler)).await
}

/// [`connect_stdio`], with a handler for what the server says unprompted —
/// `notifications/tools/list_changed`, most of all.
///
/// # Errors
///
/// [`McpError::Transport`] when the process cannot be started or its pipes
/// cannot be taken.
pub async fn connect_stdio_with(
    spec: &StdioSpec,
    handler: Arc<dyn Handler>,
) -> Result<StdioConnection, McpError> {
    let server = spec.program.clone();
    let fail = |message: String| McpError::Transport {
        server: server.clone(),
        message,
    };

    let mut command = tokio::process::Command::new(&spec.program);
    command
        .args(&spec.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Nothing outlives the connection that owns it.
        .kill_on_drop(true);
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    for (key, value) in &spec.env {
        command.env(key, value);
    }

    let mut child = command.spawn().map_err(|e| fail(e.to_string()))?;
    let stdin = child.stdin.take().ok_or_else(|| fail("no stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| fail("no stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| fail("no stderr".into()))?;

    let ring = Arc::new(StderrRing::new(STDERR_TAIL));
    tokio::spawn(orrery_jsonrpc::framing::pump_stderr(stderr, ring.clone()));

    let peer = Peer::spawn(stdout, stdin, STDIO_FRAMING, handler);
    Ok(StdioConnection {
        peer,
        child,
        stderr: ring,
    })
}

/// A streamable-HTTP MCP connection, brokered.
///
/// Every request is one `Broker::net` call: the caller supplies a fresh
/// capability token per request, because a token is single-use by construction
/// and a transport that could reuse one would be a transport that had defeated
/// the ledger.
#[derive(Clone)]
pub struct HttpTransport {
    broker: Arc<dyn Broker>,
    spec: HttpSpec,
    server: String,
}

impl std::fmt::Debug for HttpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpTransport")
            .field("server", &self.server)
            .field("url", &self.spec.url)
            .finish_non_exhaustive()
    }
}

impl HttpTransport {
    /// A transport over a broker and an endpoint.
    #[must_use]
    pub fn new(broker: Arc<dyn Broker>, server: impl Into<String>, spec: HttpSpec) -> Self {
        Self {
            broker,
            spec,
            server: server.into(),
        }
    }

    /// The host a `net(domain: ...)` rule has to allow.
    #[must_use]
    pub fn host(&self) -> String {
        NetRequest::get(self.spec.url.clone()).host()
    }

    /// POST one JSON-RPC frame and read the answer.
    ///
    /// # Errors
    ///
    /// [`McpError::Transport`] when the broker refuses the token or the request
    /// fails, and [`McpError::Malformed`] when the body is not a JSON-RPC
    /// frame.
    pub async fn send(
        &self,
        token: CapabilityToken,
        frame: &Value,
        budget: &ToolBudget,
    ) -> Result<Value, McpError> {
        let mut headers = self.spec.headers.clone();
        headers.insert("content-type".to_owned(), "application/json".to_owned());
        // Streamable HTTP: the server may answer with a single JSON body or an
        // SSE stream, and says which. We accept both and read whichever came.
        headers.insert(
            "accept".to_owned(),
            "application/json, text/event-stream".to_owned(),
        );

        let body = serde_json::to_vec(frame).map_err(|e| McpError::Transport {
            server: self.server.clone(),
            message: e.to_string(),
        })?;
        let response: NetResponse = self
            .broker
            .net(
                token,
                NetRequest::Plain {
                    method: "POST".to_owned(),
                    url: self.spec.url.clone(),
                    headers,
                    body,
                },
                budget,
            )
            .await
            .map_err(|e| McpError::Transport {
                server: self.server.clone(),
                message: e.to_string(),
            })?;

        if response.status >= 400 {
            return Err(McpError::Transport {
                server: self.server.clone(),
                message: format!("HTTP {}", response.status),
            });
        }
        parse_body(&self.server, &response)
    }
}

/// Read a streamable-HTTP answer, whichever of the two shapes it is.
///
/// A `text/event-stream` body is a sequence of `data:` lines; the last one that
/// parses as a JSON-RPC frame is the answer. A plain body is the frame.
fn parse_body(server: &str, response: &NetResponse) -> Result<Value, McpError> {
    let text = String::from_utf8_lossy(&response.body);
    let content_type = response
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.as_str())
        .unwrap_or("");

    if content_type.contains("text/event-stream") {
        let last = text
            .lines()
            .filter_map(|l| l.strip_prefix("data:"))
            .filter_map(|d| serde_json::from_str::<Value>(d.trim()).ok())
            .next_back();
        return last.ok_or_else(|| {
            McpError::malformed(
                server,
                "POST",
                "an event stream with no JSON-RPC frame in it",
            )
        });
    }

    serde_json::from_str(text.trim())
        .map_err(|e| McpError::malformed(server, "POST", e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{STDIO_FRAMING, StdioSpec};
    use orrery_jsonrpc::Framing;

    #[test]
    fn stdio_is_line_delimited_not_content_length() {
        assert_eq!(STDIO_FRAMING, Framing::LineDelimited);
        assert_ne!(STDIO_FRAMING, Framing::ContentLength);
    }

    #[test]
    fn command_text_is_what_a_spawn_rule_would_match() {
        let spec = StdioSpec::new("npx").arg("-y").arg("@acme/mcp-notes");
        assert_eq!(spec.command_text(), "npx -y @acme/mcp-notes");
    }
}
