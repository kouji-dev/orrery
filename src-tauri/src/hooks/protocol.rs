//! Pure, dependency-light parsing for the hook bridge: the minimal HTTP read,
//! the envelope `kat-hook` POSTs, event classification, and the human-readable
//! detail we surface in a permission notification. Kept free of Tauri/sockets so
//! it is trivially unit-tested.

use std::io::{self, BufRead};

use serde::Deserialize;
use serde_json::Value;

/// What `kat-hook` forwards: which agent, which tool, which hook fired, and the
/// tool's own request payload (as the agent handed it to the hook on stdin).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookEnvelope {
    pub agent_id: String,
    pub tool: String,
    pub event: String,
    #[serde(default)]
    pub payload: Value,
}

/// The three things a hook tells us, collapsed across every tool's vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    /// Blocking gate — hold and ask the user before the tool runs.
    Permission,
    /// Non-blocking status: the agent started a turn.
    Working,
    /// Non-blocking status: the agent finished a turn.
    Done,
    /// Anything else — acknowledged, no action.
    Other,
}

/// Map a tool's hook name to our event vocabulary.
pub fn classify(event: &str) -> HookEvent {
    match event {
        "PreToolUse" | "PermissionRequest" | "beforeShellExecution" | "beforeMCPExecution" => {
            HookEvent::Permission
        }
        "UserPromptSubmit" => HookEvent::Working,
        "Stop" | "stop" | "SessionEnd" => HookEvent::Done,
        _ => HookEvent::Other,
    }
}

/// A one-line, human-readable description of what the agent is about to do —
/// scraped best-effort from the tool's request payload (the command for a shell
/// call, the path for a file edit, otherwise the tool name).
pub fn summarize(env: &HookEnvelope) -> String {
    let p = &env.payload;
    // claude / codex shape: { tool_name, tool_input: { command | file_path | ... } }
    if let Some(name) = p.get("tool_name").and_then(Value::as_str) {
        let input = &p["tool_input"];
        if let Some(cmd) = input.get("command").and_then(Value::as_str) {
            return format!("{name}: {cmd}");
        }
        if let Some(path) = input.get("file_path").and_then(Value::as_str) {
            return format!("{name}: {path}");
        }
        return name.to_string();
    }
    // cursor shape: { command, cwd } for beforeShellExecution
    if let Some(cmd) = p.get("command").and_then(Value::as_str) {
        return cmd.to_string();
    }
    format!("{} · {}", env.tool, env.event)
}

/// A parsed inbound HTTP request — only the bits the bridge cares about.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// Bearer token from the `Authorization` header, if any.
    pub auth: Option<String>,
    pub body: String,
}

/// Read one HTTP/1.1 request: skip the request line, collect headers until the
/// blank line, then read exactly `Content-Length` body bytes.
pub fn read_request<R: BufRead>(r: &mut R) -> io::Result<HttpRequest> {
    let mut line = String::new();
    r.read_line(&mut line)?; // request line — ignored (always POST /)

    let mut len = 0usize;
    let mut auth = None;
    loop {
        line.clear();
        let n = r.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let t = line.trim_end();
        if t.is_empty() {
            break; // end of headers
        }
        if let Some((name, value)) = t.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => len = value.trim().parse().unwrap_or(0),
                "authorization" => {
                    auth = Some(value.trim().trim_start_matches("Bearer ").trim().to_string())
                }
                _ => {}
            }
        }
    }

    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(HttpRequest {
        auth,
        body: String::from_utf8_lossy(&buf).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn classifies_each_tools_hooks() {
        assert_eq!(classify("PreToolUse"), HookEvent::Permission);
        assert_eq!(classify("beforeShellExecution"), HookEvent::Permission);
        assert_eq!(classify("UserPromptSubmit"), HookEvent::Working);
        assert_eq!(classify("Stop"), HookEvent::Done);
        assert_eq!(classify("whatever"), HookEvent::Other);
    }

    #[test]
    fn summarizes_a_claude_bash_call() {
        let env = HookEnvelope {
            agent_id: "a".into(),
            tool: "claude".into(),
            event: "PreToolUse".into(),
            payload: serde_json::json!({
                "tool_name": "Bash",
                "tool_input": { "command": "rm -rf build" }
            }),
        };
        assert_eq!(summarize(&env), "Bash: rm -rf build");
    }

    #[test]
    fn summarizes_a_cursor_shell_call_and_falls_back() {
        let cursor = HookEnvelope {
            agent_id: "a".into(),
            tool: "cursor".into(),
            event: "beforeShellExecution".into(),
            payload: serde_json::json!({ "command": "npm test" }),
        };
        assert_eq!(summarize(&cursor), "npm test");

        let bare = HookEnvelope {
            agent_id: "a".into(),
            tool: "gemini".into(),
            event: "PreToolUse".into(),
            payload: Value::Null,
        };
        assert_eq!(summarize(&bare), "gemini · PreToolUse");
    }

    #[test]
    fn reads_an_http_post_with_auth_and_body() {
        let body = r#"{"agentId":"a1","tool":"claude","event":"PreToolUse","payload":null}"#;
        let raw = format!(
            "POST / HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer secret\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let mut c = Cursor::new(raw.into_bytes());
        let req = read_request(&mut c).unwrap();
        assert_eq!(req.auth.as_deref(), Some("secret"));
        let env: HookEnvelope = serde_json::from_str(&req.body).unwrap();
        assert_eq!(env.agent_id, "a1");
        assert_eq!(classify(&env.event), HookEvent::Permission);
    }
}
