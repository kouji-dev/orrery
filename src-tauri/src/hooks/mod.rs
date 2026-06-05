//! The hook bridge — the loopback HTTP server that the per-agent `katrix hook`
//! CLI calls into. The agents' native hooks fire it on status changes (working /
//! idle) and when the agent needs the user (permission / question); the bridge
//! relays those to the UI as events. This is the legit, hook-driven rebirth of
//! the removed `kat`/ipc. See docs/specs/2026-06-05-agent-hooks-permissions.md.
//!
//! Fire-and-forget (like stablyai/orca): every hook POST is acknowledged
//! immediately (no body), so the agent NEVER blocks on katrix — it keeps using
//! its own permission flow (you approve in its terminal). The remote allow/deny
//! round-trip is intentionally deferred; if we add it later, this is where the
//! held-request registry would live.

pub mod protocol;

use std::io::{BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Runtime};

use protocol::{classify, read_request, summarize, HookEnvelope, HookEvent};

/// Sink for events bound for the frontend — `app.emit` in production, a probe in
/// tests (keeps the server testable without a Tauri runtime).
type Emit = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// Managed Tauri state: the running loopback server. Holds the chosen port + the
/// shared token so the agent's hook can call back and be authenticated.
pub struct HookBridge {
    port: u16,
    token: String,
}

impl HookBridge {
    /// Bind a loopback server on an ephemeral port and start accepting hook
    /// callbacks. Each connection is handled on its own thread.
    pub fn start<R: Runtime>(app: AppHandle<R>) -> std::io::Result<Self> {
        Self::serve(Arc::new(move |name: &str, payload: serde_json::Value| {
            let _ = app.emit(name, payload);
        }))
    }

    /// Bind + serve with an arbitrary event sink (production passes `app.emit`).
    fn serve(emit: Emit) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let token = uuid::Uuid::new_v4().to_string();

        let token_l = token.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let emit = emit.clone();
                let token = token_l.clone();
                std::thread::spawn(move || handle(stream, &emit, &token));
            }
        });

        log::info!("hook bridge listening on 127.0.0.1:{port}");
        Ok(Self { port, token })
    }

    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    pub fn token(&self) -> &str {
        &self.token
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

fn handle(stream: TcpStream, emit: &Emit, token: &str) {
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
        let _ = write_response(&mut out, 401);
        return;
    }
    let env: HookEnvelope = match serde_json::from_str(&req.body) {
        Ok(e) => e,
        Err(_) => {
            let _ = write_response(&mut out, 204);
            return;
        }
    };

    // fire-and-forget: relay to the UI, acknowledge immediately (204). The agent
    // is never held — it proceeds with its own flow.
    match classify(&env.event) {
        HookEvent::NeedsInput => emit(
            "agent://permission",
            serde_json::json!({
                "agentId": env.agent_id,
                "tool": env.tool,
                "detail": summarize(&env),
            }),
        ),
        HookEvent::Working => emit_status(emit, &env.agent_id, "working"),
        HookEvent::Done => emit_status(emit, &env.agent_id, "idle"),
        HookEvent::Other => {}
    }
    let _ = write_response(&mut out, 204);
}

fn emit_status(emit: &Emit, agent_id: &str, state: &str) {
    emit(
        "agent://status",
        serde_json::json!({ "id": agent_id, "state": state }),
    );
}

fn write_response(out: &mut TcpStream, status: u16) -> std::io::Result<()> {
    let reason = match status {
        204 => "No Content",
        401 => "Unauthorized",
        _ => "OK",
    };
    let resp =
        format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    out.write_all(resp.as_bytes())?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Minimal `katrix hook`-style POST: returns (status_line, body).
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

    // A needs-input hook emits agent://permission and returns immediately (204),
    // never holding the agent.
    #[test]
    fn needs_input_emits_and_acks_immediately() {
        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://permission" {
                let _ = tx.send(payload["detail"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, body) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"Notification","payload":{"message":"needs permission to run Bash"}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        assert_eq!(body, "", "fire-and-forget: no decision body");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "needs permission to run Bash"
        );
    }

    // A status ping (UserPromptSubmit) emits agent://status and acks immediately.
    #[test]
    fn status_ping_emits_working() {
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
        assert!(status.contains("204"));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "working");
    }

    // A wrong token is rejected (the server only honours our own hooks).
    #[test]
    fn bad_token_is_unauthorized() {
        let bridge = HookBridge::serve(Arc::new(|_, _| {})).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            "wrong-token",
            r#"{"agentId":"a","tool":"claude","event":"Notification","payload":null}"#,
        );
        assert!(status.contains("401"), "status: {status}");
    }

    #[test]
    fn endpoint_is_loopback() {
        let bridge = HookBridge {
            port: 1234,
            token: "t".into(),
        };
        assert_eq!(bridge.endpoint(), "http://127.0.0.1:1234");
    }
}
