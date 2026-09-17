//! JSON-RPC 2.0 client over a pair of byte streams (the server's stdin/stdout)
//! on plain std threads — no async runtime. One reader thread demuxes
//! responses (by id, through the pending map), server→client requests (the
//! [`RequestHandler`] answers them inline) and notifications (fire-and-forget
//! to the [`NotifyHandler`]). Requests are synchronous with a timeout; a late
//! reply for a timed-out id finds no pending sender and is dropped. stderr is
//! drained on its own thread into a 64 KB ring so `last_error()` always has
//! the tail of the server's log.

use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use super::transport::{read_message, write_message};

/// Capacity of the stderr ring.
const STDERR_RING: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

impl RpcError {
    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {method}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LspErr {
    Timeout,
    Rpc(RpcError),
    Closed,
}

impl std::fmt::Display for LspErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LspErr::Timeout => write!(f, "timeout"),
            LspErr::Rpc(e) => write!(f, "rpc {}: {}", e.code, e.message),
            LspErr::Closed => write!(f, "connection closed"),
        }
    }
}

/// Answers a server→client request `(method, params)`.
pub type RequestHandler = Box<dyn Fn(&str, &Value) -> Result<Value, RpcError> + Send + Sync>;
/// Receives server notifications `(method, params)`.
pub type NotifyHandler = Box<dyn Fn(&str, &Value) + Send + Sync>;
/// Runs once when the reader hits EOF / an error (the server went away).
pub type CloseHandler = Box<dyn FnOnce() + Send>;

/// Fixed-capacity byte ring for the stderr tail.
struct Ring {
    buf: Vec<u8>,
    cap: usize,
}

impl Ring {
    fn push(&mut self, bytes: &[u8]) {
        if bytes.len() >= self.cap {
            self.buf.clear();
            self.buf.extend_from_slice(&bytes[bytes.len() - self.cap..]);
            return;
        }
        let overflow = (self.buf.len() + bytes.len()).saturating_sub(self.cap);
        if overflow > 0 {
            self.buf.drain(..overflow);
        }
        self.buf.extend_from_slice(bytes);
    }
}

pub struct Client {
    next_id: AtomicI64,
    pending: Mutex<HashMap<i64, Sender<Result<Value, RpcError>>>>,
    writer: Mutex<Box<dyn Write + Send>>,
    closed: AtomicBool,
    stderr: Arc<Mutex<Ring>>,
}

impl Client {
    /// Wire the streams and start the reader (+ stderr) threads.
    pub fn start(
        reader: Box<dyn Read + Send>,
        writer: Box<dyn Write + Send>,
        stderr: Option<Box<dyn Read + Send>>,
        on_request: RequestHandler,
        on_notify: NotifyHandler,
        on_close: CloseHandler,
    ) -> Arc<Client> {
        let client = Arc::new(Client {
            next_id: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
            writer: Mutex::new(writer),
            closed: AtomicBool::new(false),
            stderr: Arc::new(Mutex::new(Ring {
                buf: Vec::new(),
                cap: STDERR_RING,
            })),
        });
        if let Some(mut err) = stderr {
            let ring = client.stderr.clone();
            std::thread::Builder::new()
                .name("lsp-stderr".into())
                .spawn(move || {
                    let mut buf = [0u8; 4096];
                    loop {
                        match err.read(&mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(n) => ring.lock().unwrap().push(&buf[..n]),
                        }
                    }
                })
                .expect("spawn lsp stderr thread");
        }
        let c = client.clone();
        std::thread::Builder::new()
            .name("lsp-reader".into())
            .spawn(move || {
                let mut r = BufReader::new(reader);
                loop {
                    match read_message(&mut r) {
                        Ok(Some(body)) => c.dispatch(&body, &on_request, &on_notify),
                        Ok(None) => break,
                        Err(e) => {
                            log::debug!("lsp: reader: {e}");
                            break;
                        }
                    }
                }
                c.mark_closed();
                on_close();
            })
            .expect("spawn lsp reader thread");
        client
    }

    fn mark_closed(&self) {
        self.closed.store(true, Ordering::SeqCst);
        // every waiter learns the server is gone
        self.pending.lock().unwrap().clear();
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Tail of the server's stderr (lossy UTF-8).
    pub fn last_error(&self) -> String {
        let ring = self.stderr.lock().unwrap();
        String::from_utf8_lossy(&ring.buf).trim().to_string()
    }

    fn dispatch(&self, body: &[u8], on_request: &RequestHandler, on_notify: &NotifyHandler) {
        let msg: Value = match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(e) => {
                log::debug!("lsp: bad json from server: {e}");
                return;
            }
        };
        let id = msg.get("id").filter(|v| !v.is_null()).cloned();
        let method = msg.get("method").and_then(Value::as_str).map(str::to_string);
        match (id, method) {
            (Some(id), Some(method)) => {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                let reply = match on_request(&method, &params) {
                    Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    Err(e) => json!({
                        "jsonrpc": "2.0", "id": id,
                        "error": { "code": e.code, "message": e.message }
                    }),
                };
                let _ = self.send(&reply);
            }
            (Some(id), None) => {
                let Some(id) = id.as_i64() else {
                    return; // we only ever send integer ids
                };
                let outcome = match msg.get("error") {
                    Some(e) if !e.is_null() => Err(RpcError {
                        code: e.get("code").and_then(Value::as_i64).unwrap_or(-32000),
                        message: e
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    }),
                    _ => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                };
                // A late reply for a timed-out id has no sender any more → dropped.
                if let Some(tx) = self.pending.lock().unwrap().remove(&id) {
                    let _ = tx.send(outcome);
                }
            }
            (None, Some(method)) => {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                on_notify(&method, &params);
            }
            (None, None) => log::debug!("lsp: message without id or method"),
        }
    }

    fn send(&self, msg: &Value) -> Result<(), LspErr> {
        if self.is_closed() {
            return Err(LspErr::Closed);
        }
        let body = serde_json::to_vec(msg).map_err(|_| LspErr::Closed)?;
        let mut w = self.writer.lock().unwrap();
        write_message(&mut *w, &body).map_err(|e| {
            log::debug!("lsp: write: {e}");
            LspErr::Closed
        })
    }

    /// Synchronous request. On timeout the id is forgotten and a
    /// `$/cancelRequest` goes out; a reply arriving later is dropped.
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, LspErr> {
        if self.is_closed() {
            return Err(LspErr::Closed);
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let msg = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        if let Err(e) = self.send(&msg) {
            self.pending.lock().unwrap().remove(&id);
            return Err(e);
        }
        match rx.recv_timeout(timeout) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(e)) => Err(LspErr::Rpc(e)),
            Err(RecvTimeoutError::Timeout) => {
                self.pending.lock().unwrap().remove(&id);
                let _ = self.notify("$/cancelRequest", json!({ "id": id }));
                Err(LspErr::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => Err(LspErr::Closed),
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), LspErr> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }
}

/// In-memory duplex streams for tests + the fake server used by the router
/// tests: a `Read` fed by a channel and a `Write` that forwards whole chunks.
#[cfg(test)]
pub(crate) mod fake {
    use super::*;
    use std::io;
    use std::sync::mpsc::Receiver;

    pub struct ChanReader {
        rx: Receiver<Vec<u8>>,
        buf: Vec<u8>,
        pos: usize,
    }

    impl Read for ChanReader {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.pos >= self.buf.len() {
                match self.rx.recv() {
                    Ok(b) => {
                        self.buf = b;
                        self.pos = 0;
                    }
                    Err(_) => return Ok(0), // sender gone = EOF
                }
            }
            let n = out.len().min(self.buf.len() - self.pos);
            out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
            self.pos += n;
            Ok(n)
        }
    }

    pub struct ChanWriter(pub Sender<Vec<u8>>);

    impl Write for ChanWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .send(buf.to_vec())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "reader gone"))?;
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// `(client side reader, client side writer, server side reader, server side writer)`.
    pub fn duplex() -> (ChanReader, ChanWriter, ChanReader, ChanWriter) {
        let (to_client_tx, to_client_rx) = mpsc::channel();
        let (to_server_tx, to_server_rx) = mpsc::channel();
        (
            ChanReader { rx: to_client_rx, buf: Vec::new(), pos: 0 },
            ChanWriter(to_server_tx),
            ChanReader { rx: to_server_rx, buf: Vec::new(), pos: 0 },
            ChanWriter(to_client_tx),
        )
    }

    /// What the fake server does with each incoming message.
    pub type Script = Box<dyn FnMut(&Value, &mut dyn FnMut(Value)) + Send>;

    /// Start a fake server: `script(msg, reply)` runs per message on the
    /// server thread; `reply` writes to the client. Returns the client.
    pub fn client_with(
        mut script: Script,
        on_request: RequestHandler,
        on_notify: NotifyHandler,
        on_close: CloseHandler,
    ) -> Arc<Client> {
        let (cr, cw, sr, sw) = duplex();
        std::thread::spawn(move || {
            let mut r = BufReader::new(sr);
            let sw = Arc::new(Mutex::new(sw));
            while let Ok(Some(body)) = read_message(&mut r) {
                let msg: Value = serde_json::from_slice(&body).unwrap();
                let sw2 = sw.clone();
                let mut reply = move |v: Value| {
                    let _ = write_message(&mut *sw2.lock().unwrap(), &serde_json::to_vec(&v).unwrap());
                };
                script(&msg, &mut reply);
            }
        });
        Client::start(Box::new(cr), Box::new(cw), None, on_request, on_notify, on_close)
    }

    /// A server that answers every request with `result` after `delay`
    /// (notifications are swallowed; `initialize` gets empty capabilities).
    pub fn delayed_server(delay: Duration, result: Value) -> Arc<Client> {
        client_with(
            Box::new(move |msg, reply| {
                let Some(id) = msg.get("id").cloned() else {
                    return;
                };
                let result = if msg["method"] == "initialize" {
                    json!({ "capabilities": {} })
                } else {
                    result.clone()
                };
                std::thread::sleep(delay);
                reply(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
            }),
            Box::new(|m, _| Err(RpcError::method_not_found(m))),
            Box::new(|_, _| {}),
            Box::new(|| {}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::fake::*;
    use super::*;

    fn silent() -> (RequestHandler, NotifyHandler, CloseHandler) {
        (
            Box::new(|m, _| Err(RpcError::method_not_found(m))),
            Box::new(|_, _| {}),
            Box::new(|| {}),
        )
    }

    #[test]
    fn correlates_replies_by_id_out_of_order() {
        // the server answers the SECOND request first
        let (rq, nt, cl) = silent();
        let client = client_with(
            Box::new(|msg, reply| {
                let id = msg["id"].as_i64().unwrap();
                if id == 1 {
                    // hold it: answer 1 after 2 by delaying on this thread
                    std::thread::sleep(Duration::from_millis(60));
                }
                reply(json!({ "jsonrpc": "2.0", "id": id, "result": { "echo": msg["params"] } }));
            }),
            rq,
            nt,
            cl,
        );
        let c2 = client.clone();
        let t = std::thread::spawn(move || c2.request("b", json!(2), Duration::from_secs(2)));
        let r1 = client.request("a", json!(1), Duration::from_secs(2)).unwrap();
        let r2 = t.join().unwrap().unwrap();
        assert_eq!(r1["echo"], 1);
        assert_eq!(r2["echo"], 2);
    }

    #[test]
    fn timeout_cancels_and_drops_the_late_reply() {
        let (rq, nt, cl) = silent();
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let client = client_with(
            Box::new(move |msg, reply| {
                let method = msg["method"].as_str().unwrap_or("").to_string();
                seen2.lock().unwrap().push(method.clone());
                if method == "slow" {
                    let id = msg["id"].clone();
                    std::thread::sleep(Duration::from_millis(120));
                    reply(json!({ "jsonrpc": "2.0", "id": id, "result": "late" }));
                } else if let Some(id) = msg.get("id") {
                    reply(json!({ "jsonrpc": "2.0", "id": id, "result": "fast" }));
                }
            }),
            rq,
            nt,
            cl,
        );
        let r = client.request("slow", Value::Null, Duration::from_millis(30));
        assert_eq!(r, Err(LspErr::Timeout));
        // the late reply must not be delivered to the NEXT request
        let r = client.request("fast", Value::Null, Duration::from_secs(2)).unwrap();
        assert_eq!(r, "fast");
        std::thread::sleep(Duration::from_millis(150));
        assert!(client.pending.lock().unwrap().is_empty());
        let seen = seen.lock().unwrap().clone();
        assert!(seen.contains(&"$/cancelRequest".to_string()), "{seen:?}");
    }

    #[test]
    fn answers_server_requests_and_routes_notifications() {
        let notes: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let notes2 = notes.clone();
        let client = client_with(
            Box::new(|msg, reply| {
                if msg["method"] == "ping" {
                    let id = msg["id"].clone();
                    // ask the client something first, then a notification, then answer
                    reply(json!({ "jsonrpc": "2.0", "id": 900, "method": "workspace/configuration", "params": { "items": [{ "section": "java" }] } }));
                    reply(json!({ "jsonrpc": "2.0", "id": 901, "method": "unknown/thing", "params": {} }));
                    reply(json!({ "jsonrpc": "2.0", "method": "window/showMessage", "params": { "type": 3, "message": "hi" } }));
                    reply(json!({ "jsonrpc": "2.0", "id": id, "result": null }));
                } else if msg.get("id").is_some() && msg.get("method").is_none() {
                    // a response to OUR request: echo it back as a notification so the test can see it
                    reply(json!({ "jsonrpc": "2.0", "method": "echo/response", "params": msg }));
                }
            }),
            Box::new(|method, params| match method {
                "workspace/configuration" => Ok(json!(params["items"].as_array().map(|a| a.len()).unwrap_or(0))),
                m => Err(RpcError::method_not_found(m)),
            }),
            Box::new(move |method, params| {
                notes2.lock().unwrap().push(format!("{method}:{params}"));
            }),
            Box::new(|| {}),
        );
        let r = client.request("ping", Value::Null, Duration::from_secs(2)).unwrap();
        assert!(r.is_null());
        std::thread::sleep(Duration::from_millis(100));
        let notes = notes.lock().unwrap().clone();
        assert!(notes.iter().any(|n| n.starts_with("window/showMessage:")), "{notes:?}");
        // our answer to id 900 carried the item count; 901 got MethodNotFound
        assert!(notes.iter().any(|n| n.contains(r#""id":900"#) && n.contains(r#""result":1"#)), "{notes:?}");
        assert!(notes.iter().any(|n| n.contains(r#""id":901"#) && n.contains("-32601")), "{notes:?}");
    }

    #[test]
    fn eof_closes_and_fails_pending_requests() {
        let closed = Arc::new(AtomicBool::new(false));
        let closed2 = closed.clone();
        let (rq, nt, _) = silent();
        let (cr, cw, sr, sw) = duplex();
        let client = Client::start(
            Box::new(cr),
            Box::new(cw),
            None,
            rq,
            nt,
            Box::new(move || closed2.store(true, Ordering::SeqCst)),
        );
        let c2 = client.clone();
        let t = std::thread::spawn(move || c2.request("x", Value::Null, Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(30));
        drop(sw); // server side hangs up
        drop(sr);
        assert_eq!(t.join().unwrap(), Err(LspErr::Closed));
        std::thread::sleep(Duration::from_millis(20));
        assert!(closed.load(Ordering::SeqCst));
        assert!(client.is_closed());
        assert_eq!(client.request("y", Value::Null, Duration::from_secs(1)), Err(LspErr::Closed));
    }

    #[test]
    fn stderr_ring_keeps_the_tail() {
        let mut ring = Ring { buf: Vec::new(), cap: 8 };
        ring.push(b"abc");
        ring.push(b"defgh");
        assert_eq!(ring.buf, b"abcdefgh");
        ring.push(b"XY");
        assert_eq!(ring.buf, b"cdefghXY");
        ring.push(b"0123456789ab");
        assert_eq!(ring.buf, b"456789ab");
    }
}
