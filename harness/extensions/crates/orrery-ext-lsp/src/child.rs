//! The real transport: a language server in a child process.
//!
//! One reader thread owns the server's stdout, splits it with
//! [`crate::framing`], and routes each message by shape: a frame with an `id`
//! and a `result` or `error` completes a pending request; a frame with a
//! `method` and no `id` is a notification and is queued for
//! [`LspTransport::drain_notifications`]; a frame with a `method` **and** an id
//! is the server calling *us*, which this client answers with a
//! `MethodNotFound` rather than ignoring — a server waiting forever on
//! `workspace/configuration` is the most common way a language server appears
//! to hang.

use std::collections::HashMap;
use std::io::BufReader;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde_json::{Value, json};
use tokio::sync::oneshot;

use crate::client::{LspError, LspTransport};
use crate::framing::{read_message, write_message};

/// How long a request waits before it is reported as a timeout.
///
/// A ceiling rather than "forever": a tool call that never returns takes its
/// turn with it, and a language server indexing a large tree genuinely does go
/// quiet for minutes.
const DEFAULT_TIMEOUT_MS: u64 = 30_000;


/// The server's stdin, shared: the reader thread needs it to answer a
/// server-to-client request without waiting for anybody.
type Stdin = Arc<Mutex<Option<std::process::ChildStdin>>>;

/// A language server in a child process.
pub struct ChildTransport {
    stdin: Stdin,
    child: Mutex<Option<Child>>,
    pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, LspError>>>>>,
    notifications: Arc<Mutex<Vec<(String, Value)>>>,
    timeout_ms: u64,
}

impl std::fmt::Debug for ChildTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildTransport")
            .field("pending", &self.pending.lock().len())
            .finish_non_exhaustive()
    }
}

impl ChildTransport {
    /// Start a server.
    ///
    /// # Errors
    ///
    /// [`LspError::NotRunning`] when the program could not be started — which
    /// is the normal case for a language server that is simply not installed,
    /// and is why the tools report it rather than panicking.
    pub fn spawn(
        program: &str,
        args: &[String],
        cwd: Option<&std::path::Path>,
    ) -> Result<Self, LspError> {
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Inherited, not piped: a piped stderr nobody reads fills its pipe
            // buffer and blocks the server mid-sentence.
            .stderr(Stdio::null());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let mut child = command
            .spawn()
            .map_err(|e| LspError::NotRunning(format!("{program}: {e}")))?;

        let stdin: Stdin = Arc::new(Mutex::new(child.stdin.take()));
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| LspError::NotRunning(format!("{program}: no stdout")))?;

        let pending: Arc<Mutex<HashMap<i64, oneshot::Sender<Result<Value, LspError>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let notifications = Arc::new(Mutex::new(Vec::new()));

        let reader_pending = Arc::clone(&pending);
        let reader_notifications = Arc::clone(&notifications);
        let reader_stdin = Arc::clone(&stdin);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                match read_message(&mut reader) {
                    Ok(Some(body)) => {
                        let Ok(frame) = serde_json::from_slice::<Value>(&body) else {
                            continue;
                        };
                        route(&frame, &reader_pending, &reader_notifications, &reader_stdin);
                    }
                    // A clean end, or a broken one. Either way every pending
                    // request must be failed, or its caller waits out its whole
                    // timeout for an answer that is never coming.
                    Ok(None) | Err(_) => {
                        let waiting: Vec<_> = reader_pending.lock().drain().collect();
                        for (_, tx) in waiting {
                            let _ = tx.send(Err(LspError::NotRunning(
                                "the language server exited".to_owned(),
                            )));
                        }
                        return;
                    }
                }
            }
        });

        Ok(Self {
            stdin,
            child: Mutex::new(Some(child)),
            pending,
            notifications,
            timeout_ms: DEFAULT_TIMEOUT_MS,
        })
    }

    /// Wait a different length of time for each request.
    #[must_use]
    pub const fn with_timeout_ms(mut self, ms: u64) -> Self {
        self.timeout_ms = ms;
        self
    }

    fn send(&self, frame: &Value) -> Result<(), LspError> {
        let body = serde_json::to_vec(frame)
            .map_err(|e| LspError::Transport(format!("could not encode a frame: {e}")))?;
        let mut held = self.stdin.lock();
        let stdin = held
            .as_mut()
            .ok_or_else(|| LspError::NotRunning("stdin is closed".to_owned()))?;
        write_message(stdin, &body)
            .map_err(|e| LspError::Transport(format!("could not write to the server: {e}")))
    }
}

/// One server-to-client frame, by shape.
fn route(
    frame: &Value,
    pending: &Mutex<HashMap<i64, oneshot::Sender<Result<Value, LspError>>>>,
    notifications: &Mutex<Vec<(String, Value)>>,
    stdin: &Stdin,
) {
    let method = frame["method"].as_str();
    let id = frame["id"].as_i64();
    match (method, id) {
        // A response to something we asked.
        (None, Some(id)) => {
            let Some(tx) = pending.lock().remove(&id) else {
                return;
            };
            let answer = if frame["error"].is_null() {
                Ok(frame["result"].clone())
            } else {
                Err(LspError::Refused {
                    method: format!("request {id}"),
                    message: frame["error"]["message"]
                        .as_str()
                        .unwrap_or("no message")
                        .to_owned(),
                })
            };
            let _ = tx.send(answer);
        }
        // A notification: diagnostics, progress, log messages.
        (Some(method), None) => {
            notifications
                .lock()
                .push((method.to_owned(), frame["params"].clone()));
        }
        // The server calling *us*. Answered, not ignored: a server waiting
        // forever on `workspace/configuration` is the most common way a
        // language server appears to hang. This client implements none of the
        // reverse methods, and `-32601` is how the protocol says so.
        (Some(_), Some(id)) => {
            let reply = json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": "this client implements no server-to-client requests",
                },
            });
            if let Ok(body) = serde_json::to_vec(&reply) {
                let mut held = stdin.lock();
                if let Some(stdin) = held.as_mut() {
                    let _ = write_message(stdin, &body);
                }
            }
        }
        // Neither a method nor an id: not a JSON-RPC frame at all.
        (None, None) => {}
    }
}

#[async_trait]
impl LspTransport for ChildTransport {
    async fn request(&self, id: i64, method: &str, params: Value) -> Result<Value, LspError> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        let frame = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if let Err(e) = self.send(&frame) {
            self.pending.lock().remove(&id);
            return Err(e);
        }
        match tokio::time::timeout(Duration::from_millis(self.timeout_ms), rx).await {
            Ok(Ok(answer)) => answer,
            Ok(Err(_)) => Err(LspError::NotRunning(
                "the language server exited".to_owned(),
            )),
            Err(_) => {
                // Drop the slot, or a slow answer arriving later completes a
                // request nobody is waiting for and leaks the sender.
                self.pending.lock().remove(&id);
                Err(LspError::TimedOut {
                    method: method.to_owned(),
                    ms: self.timeout_ms,
                })
            }
        }
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), LspError> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    async fn drain_notifications(&self) -> Vec<(String, Value)> {
        std::mem::take(&mut *self.notifications.lock())
    }
}

impl Drop for ChildTransport {
    /// Close stdin and kill the child.
    ///
    /// A language server whose client goes away does **not** reliably exit on
    /// its own — several index in the background and never look at stdin again
    /// — and a leaked one holds a CPU for the rest of the session.
    fn drop(&mut self) {
        *self.stdin.lock() = None;
        if let Some(mut child) = self.child.lock().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
