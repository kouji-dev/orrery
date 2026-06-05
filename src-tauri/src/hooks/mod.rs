//! The hook bridge — the loopback HTTP server that the per-agent `kat-hook`
//! helper calls back into. A blocking pre-tool hook lets us drive BOTH reliable
//! status detection AND the reverse path: hold the agent's tool call, ask the
//! user (via a notification), and forward the allow/deny decision back so the
//! agent proceeds — no fake keystroke. This is the legit, hook-driven rebirth of
//! the removed `kat`/ipc. See docs/specs/2026-06-05-agent-hooks-permissions.md.

pub mod protocol;

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Runtime};
use uuid::Uuid;

use crate::agents::adapters::{self, Decision};
use protocol::{classify, read_request, summarize, HookEnvelope, HookEvent};

/// How long a blocking permission request waits for the user before falling back
/// to the agent's own prompt. Under Claude's 600s hook timeout.
const DECIDE_TIMEOUT: Duration = Duration::from_secs(590);

type Pending = Arc<Mutex<HashMap<Uuid, Sender<Decision>>>>;
/// Sink for events bound for the frontend — `app.emit` in production, a probe in
/// tests (keeps the server testable without a Tauri runtime).
type Emit = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// Managed Tauri state: the running loopback server + the held-request registry.
pub struct HookBridge {
    port: u16,
    token: String,
    pending: Pending,
}

impl HookBridge {
    /// Bind a loopback server on an ephemeral port and start accepting hook
    /// callbacks. Each connection is handled on its own thread so a blocking
    /// permission request never stalls another agent's hook.
    pub fn start<R: Runtime>(app: AppHandle<R>) -> std::io::Result<Self> {
        Self::serve(Arc::new(move |name: &str, payload: serde_json::Value| {
            let _ = app.emit(name, payload);
        }))
    }

    /// Bind + serve with an arbitrary event sink (production passes `app.emit`).
    fn serve(emit: Emit) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let token = Uuid::new_v4().to_string();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));

        let token_l = token.clone();
        let pending_l = pending.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let emit = emit.clone();
                let token = token_l.clone();
                let pending = pending_l.clone();
                std::thread::spawn(move || {
                    handle(stream, &emit, &token, &pending);
                });
            }
        });

        log::info!("hook bridge listening on 127.0.0.1:{port}");
        Ok(Self { port, token, pending })
    }

    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Resolve a held permission request with the user's decision. Wakes the
    /// blocked hook connection so it returns the decision JSON to the agent.
    pub fn decide(&self, request_id: Uuid, allow: bool) {
        if let Some(tx) = self.pending.lock().unwrap().get(&request_id) {
            let _ = tx.send(Decision::from_allow(allow));
        }
    }
}

/// The executable agents invoke for their hooks — the katrix app itself, re-run
/// as `katrix hook --event <EVENT>` (see cli::hook). Always co-located with the
/// running app, so no separate sidecar to bundle. `None` only if the OS can't
/// report our own path, in which case the agent launches without hooks (PTY
/// parsing fallback).
pub fn hook_binary() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

fn handle(stream: TcpStream, emit: &Emit, token: &str, pending: &Pending) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut out = stream;

    let req = match read_request(&mut reader) {
        Ok(r) => r,
        Err(_) => return,
    };
    if req.auth.as_deref() != Some(token) {
        let _ = write_response(&mut out, 401, "");
        return;
    }
    let env: HookEnvelope = match serde_json::from_str(&req.body) {
        Ok(e) => e,
        // unparseable → no opinion (agent proceeds normally)
        Err(_) => {
            let _ = write_response(&mut out, 200, "");
            return;
        }
    };

    match classify(&env.event) {
        HookEvent::Permission => {
            let body = hold_for_decision(emit, &env, pending);
            let _ = write_response(&mut out, 200, &body);
        }
        HookEvent::Working => {
            emit_status(emit, &env.agent_id, "working");
            let _ = write_response(&mut out, 200, "");
        }
        HookEvent::Done => {
            emit_status(emit, &env.agent_id, "idle");
            let _ = write_response(&mut out, 200, "");
        }
        HookEvent::Other => {
            let _ = write_response(&mut out, 200, "");
        }
    }
}

/// Register a held request, ask the UI, and block until the user decides (or the
/// timeout falls back to the agent's own prompt). Returns the tool-specific
/// decision JSON for `kat-hook` to print.
fn hold_for_decision(emit: &Emit, env: &HookEnvelope, pending: &Pending) -> String {
    let request_id = Uuid::new_v4();
    let (tx, rx) = mpsc::channel::<Decision>();
    pending.lock().unwrap().insert(request_id, tx);

    let detail = summarize(env);
    emit(
        "agent://permission",
        serde_json::json!({
            "requestId": request_id.to_string(),
            "agentId": env.agent_id,
            "tool": env.tool,
            "detail": detail,
        }),
    );

    let decision = rx.recv_timeout(DECIDE_TIMEOUT).unwrap_or(Decision::Ask);
    pending.lock().unwrap().remove(&request_id);

    let reason = match decision {
        Decision::Allow => "approved from katrix",
        Decision::Deny => "rejected from katrix",
        Decision::Ask => "no response — deferring to the agent's own prompt",
    };
    adapters::adapter_for(&env.tool)
        .map(|a| a.format_decision(decision, reason))
        .unwrap_or_default()
}

fn emit_status(emit: &Emit, agent_id: &str, state: &str) {
    emit(
        "agent://status",
        serde_json::json!({ "id": agent_id, "state": state }),
    );
}

fn write_response(out: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let reason = if status == 200 { "OK" } else { "Unauthorized" };
    let resp = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    out.write_all(resp.as_bytes())?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::net::TcpStream;

    /// Minimal `kat-hook`-style POST: returns (status_line, body).
    fn post(endpoint: &str, token: &str, body: &str) -> (String, String) {
        let addr = endpoint.trim_start_matches("http://");
        let mut s = TcpStream::connect(addr).unwrap();
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).unwrap();
        s.flush().unwrap();
        let mut resp = String::new();
        s.read_to_string(&mut resp).unwrap();
        let status = resp.lines().next().unwrap_or("").to_string();
        let body = resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status, body)
    }

    // Full loopback round-trip: a hook POSTs a permission request, the bridge
    // holds it and emits to the UI; once decide() fires, the held connection
    // returns the tool's allow JSON — exactly what kat-hook prints to the agent.
    #[test]
    fn permission_round_trip_returns_the_decision() {
        let (tx, rx) = mpsc::channel::<String>(); // capture the emitted requestId
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://permission" {
                let _ = tx.send(payload["requestId"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let endpoint = bridge.endpoint();
        let token = bridge.token().to_string();

        // the hook call blocks until a decision, so issue it from another thread
        let h = std::thread::spawn(move || {
            post(
                &endpoint,
                &token,
                r#"{"agentId":"a1","tool":"claude","event":"PreToolUse","payload":{"tool_name":"Bash","tool_input":{"command":"ls"}}}"#,
            )
        });

        // the UI received the request → approve it
        let request_id = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        bridge.decide(Uuid::parse_str(&request_id).unwrap(), true);

        let (status, body) = h.join().unwrap();
        assert!(status.contains("200"), "status: {status}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["hookSpecificOutput"]["permissionDecision"], "allow");
    }

    // A status ping (UserPromptSubmit) returns immediately and emits agent://status.
    #[test]
    fn status_ping_returns_immediately() {
        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://status" {
                let _ = tx.send(payload["state"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"UserPromptSubmit","payload":null}"#,
        );
        assert!(status.contains("200"));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "working");
    }

    // A wrong token is rejected (the server only honours our own hooks).
    #[test]
    fn bad_token_is_unauthorized() {
        let bridge = HookBridge::serve(Arc::new(|_, _| {})).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            "wrong-token",
            r#"{"agentId":"a","tool":"claude","event":"PreToolUse","payload":null}"#,
        );
        assert!(status.contains("401"), "status: {status}");
    }

    // The decide → pending registry contract, independent of HTTP/Tauri: a held
    // request is woken with the user's decision; an unknown id is a harmless no-op.
    #[test]
    fn decide_wakes_a_held_request() {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let bridge = HookBridge {
            port: 0,
            token: "t".into(),
            pending: pending.clone(),
        };
        let id = Uuid::new_v4();
        let (tx, rx) = mpsc::channel::<Decision>();
        pending.lock().unwrap().insert(id, tx);

        bridge.decide(id, true);
        assert_eq!(rx.recv_timeout(Duration::from_secs(1)).unwrap(), Decision::Allow);

        // unknown id: no panic, nothing delivered
        bridge.decide(Uuid::new_v4(), false);
    }

    #[test]
    fn endpoint_is_loopback() {
        let bridge = HookBridge {
            port: 1234,
            token: "t".into(),
            pending: Arc::new(Mutex::new(HashMap::new())),
        };
        assert_eq!(bridge.endpoint(), "http://127.0.0.1:1234");
    }
}
