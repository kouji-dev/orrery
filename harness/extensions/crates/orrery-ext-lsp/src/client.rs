//! A managed language-server client: the seam, and the bookkeeping over it.
//!
//! # The seam
//!
//! [`LspTransport`] is what a server is reached through. It is a trait because
//! a test must not need `rust-analyzer`, `gopls` or `pyright` installed — the
//! machine running this suite has none of them, and a test that needed one
//! would be a test that gets deleted the first time it is flaky.
//!
//! [`ChildTransport`] is the real one: a process, `Content-Length` framing over
//! its stdin and stdout, and a reader thread that correlates responses to
//! requests by id.
//!
//! # The bookkeeping
//!
//! [`LspClient`] owns what LSP requires and a tool caller should not have to
//! remember: `initialize` exactly once, `textDocument/didOpen` before a
//! position request, and the diagnostics that arrive as **notifications** at
//! whatever moment the server feels like rather than as an answer to anything.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{Value, json};

/// A language server did not answer, or answered badly.
#[derive(Debug, Clone, thiserror::Error)]
pub enum LspError {
    /// The server could not be started or has gone.
    #[error("the language server is not running: {0}")]
    NotRunning(String),
    /// The server answered with a JSON-RPC error object.
    #[error("{method} failed: {message}")]
    Refused {
        /// Which request.
        method: String,
        /// The server's own words.
        message: String,
    },
    /// The transport broke.
    #[error("{0}")]
    Transport(String),
    /// The server took too long.
    #[error("`{method}` did not answer within {ms}ms")]
    TimedOut {
        /// Which request.
        method: String,
        /// How long it had.
        ms: u64,
    },
}

/// How a language server is reached.
///
/// Requests and notifications are separate methods rather than one `send`,
/// because the difference is the whole protocol: a notification has no id and
/// **must not** be waited for. A transport that waited for one would hang on
/// every `didOpen`.
#[async_trait]
pub trait LspTransport: Send + Sync {
    /// Send a request and wait for its response.
    ///
    /// # Errors
    ///
    /// [`LspError`] when the server refused, broke or did not answer.
    async fn request(&self, id: i64, method: &str, params: Value) -> Result<Value, LspError>;

    /// Send a notification. There is no answer and none is waited for.
    ///
    /// # Errors
    ///
    /// [`LspError`] when the transport broke.
    async fn notify(&self, method: &str, params: Value) -> Result<(), LspError>;

    /// Take every server-to-client notification that has arrived since the last
    /// call, in order.
    ///
    /// Diagnostics are **pushed**, not requested: they arrive when the server
    /// has finished thinking, which is not when anybody asked. Draining is how
    /// a request/response tool reads a push protocol without inventing a
    /// `textDocument/getDiagnostics` that no server implements.
    async fn drain_notifications(&self) -> Vec<(String, Value)>;
}

/// One open document, as the server was told about it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct OpenDoc {
    version: i64,
}

/// A language server, plus what LSP requires a client to remember.
pub struct LspClient {
    transport: Arc<dyn LspTransport>,
    next_id: AtomicI64,
    initialized: Mutex<bool>,
    open: Mutex<HashMap<String, OpenDoc>>,
    /// The last diagnostics published per document uri. Latest wins: the server
    /// republishes the **whole** set for a file, so appending would show a
    /// person errors they have already fixed.
    diagnostics: Mutex<HashMap<String, Vec<Value>>>,
    root_uri: String,
}

impl std::fmt::Debug for LspClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspClient")
            .field("root_uri", &self.root_uri)
            .field("initialized", &*self.initialized.lock())
            .field("open", &self.open.lock().len())
            .finish_non_exhaustive()
    }
}

impl LspClient {
    /// A client over a transport, rooted at a workspace.
    #[must_use]
    pub fn new(transport: Arc<dyn LspTransport>, root_uri: impl Into<String>) -> Self {
        Self {
            transport,
            next_id: AtomicI64::new(1),
            initialized: Mutex::new(false),
            open: Mutex::new(HashMap::new()),
            diagnostics: Mutex::new(HashMap::new()),
            root_uri: root_uri.into(),
        }
    }

    /// `initialize` then `initialized`, exactly once however many tools run.
    ///
    /// # Errors
    ///
    /// [`LspError`] when the server refused the handshake.
    pub async fn ensure_initialized(&self) -> Result<(), LspError> {
        // Checked and set under one lock so two concurrent tool calls cannot
        // both decide they are the first. A server sent two `initialize`s
        // answers the second with an error and is then unusable.
        {
            let mut done = self.initialized.lock();
            if *done {
                return Ok(());
            }
            *done = true;
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let result = self
            .transport
            .request(
                id,
                "initialize",
                json!({
                    "processId": Value::Null,
                    "rootUri": self.root_uri,
                    "capabilities": {
                        "textDocument": {
                            "hover": { "contentFormat": ["markdown", "plaintext"] },
                            "definition": { "linkSupport": true },
                            "references": {},
                            "publishDiagnostics": {},
                        },
                    },
                }),
            )
            .await;
        if let Err(e) = result {
            // Put it back, so a transient failure is retried rather than
            // leaving the client permanently "initialized" against a server
            // that never was.
            *self.initialized.lock() = false;
            return Err(e);
        }
        self.transport.notify("initialized", json!({})).await
    }

    /// Tell the server about a document, once per version.
    ///
    /// # Errors
    ///
    /// [`LspError`] when the notification could not be sent.
    pub async fn open(&self, uri: &str, language_id: &str, text: &str) -> Result<(), LspError> {
        self.ensure_initialized().await?;
        let version = {
            let mut open = self.open.lock();
            match open.get_mut(uri) {
                Some(doc) => {
                    doc.version += 1;
                    Some(doc.version)
                }
                None => {
                    open.insert(uri.to_owned(), OpenDoc { version: 1 });
                    None
                }
            }
        };
        match version {
            // Already open: a second `didOpen` is a protocol error on most
            // servers, so a re-read is a change.
            Some(version) => {
                self.transport
                    .notify(
                        "textDocument/didChange",
                        json!({
                            "textDocument": { "uri": uri, "version": version },
                            "contentChanges": [{ "text": text }],
                        }),
                    )
                    .await
            }
            None => {
                self.transport
                    .notify(
                        "textDocument/didOpen",
                        json!({
                            "textDocument": {
                                "uri": uri,
                                "languageId": language_id,
                                "version": 1,
                                "text": text,
                            },
                        }),
                    )
                    .await
            }
        }
    }

    /// One request, with the handshake already done.
    ///
    /// # Errors
    ///
    /// [`LspError`] as the transport reports it.
    pub async fn request(&self, method: &str, params: Value) -> Result<Value, LspError> {
        self.ensure_initialized().await?;
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let out = self.transport.request(id, method, params).await;
        // Whatever arrived while we waited. A server publishes diagnostics as
        // it finishes analysing, which is usually while something else is in
        // flight — collecting them only inside `diagnostics` would miss them.
        self.collect().await;
        out
    }

    /// Whatever the server has published for a document, latest set only.
    pub async fn diagnostics(&self, uri: &str) -> Vec<Value> {
        self.collect().await;
        self.diagnostics.lock().get(uri).cloned().unwrap_or_default()
    }

    /// Move pushed notifications into the per-document store.
    async fn collect(&self) {
        for (method, params) in self.transport.drain_notifications().await {
            if method != "textDocument/publishDiagnostics" {
                continue;
            }
            let Some(uri) = params["uri"].as_str() else {
                continue;
            };
            let items = params["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            // Replace, never append: a publish is the **whole** set for that
            // file, so appending would keep showing errors already fixed.
            self.diagnostics.lock().insert(uri.to_owned(), items);
        }
    }

    /// Ask the server to stop. Best effort: a server that has already died is
    /// not a failure to report.
    pub async fn shutdown(&self) {
        if !*self.initialized.lock() {
            return;
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let _ = self.transport.request(id, "shutdown", Value::Null).await;
        let _ = self.transport.notify("exit", Value::Null).await;
    }
}
