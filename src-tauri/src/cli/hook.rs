//! The `hook` subcommand — the client half of the hook bridge. An installed agent
//! hook runs `katrix hook --event <EVENT>`; this resolves the call (flags, then
//! KATRIX_* env, then stdin for the payload), POSTs it to the loopback bridge,
//! waits, and prints the decision JSON the agent then obeys. std-only transport,
//! so the hot hook path stays light.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

use clap::Args;

/// `katrix hook` flags. Only `--event` is required (it varies per hook entry);
/// connection + identity default to the KATRIX_* env we stamp on the agent
/// process, and the payload defaults to stdin (how agents deliver the request).
/// The remaining flags exist so the CLI is fully self-contained for testing.
#[derive(Args)]
pub struct HookArgs {
    /// Hook event (PreToolUse, UserPromptSubmit, Stop, beforeShellExecution, …).
    #[arg(long)]
    event: String,
    /// Agent tool id (default: $KATRIX_TOOL).
    #[arg(long)]
    tool: Option<String>,
    /// Agent id (default: $KATRIX_AGENT_ID).
    #[arg(long = "agent-id")]
    agent_id: Option<String>,
    /// Bridge endpoint, e.g. http://127.0.0.1:PORT (default: $KATRIX_ENDPOINT).
    #[arg(long)]
    endpoint: Option<String>,
    /// Auth token (default: $KATRIX_TOKEN).
    #[arg(long)]
    token: Option<String>,
    /// Tool request JSON (default: read stdin — the agent's hook protocol).
    #[arg(long)]
    payload: Option<String>,
}

/// The shell command an agent runs for one hook event: the app exe re-invoked as
/// `katrix hook --event <EVENT>`. Connection + identity flow via the KATRIX_* env
/// we stamp on the agent process. Quoted so a path with spaces is safe.
pub fn hook_command(exe: &Path, event: &str) -> String {
    format!("\"{}\" {} --event {}", exe.display(), super::SUBCOMMANDS[0], event)
}

/// Run the `hook` subcommand: resolve inputs, broker with the bridge, print the
/// decision. No bridge / no response = print nothing, so the agent proceeds with
/// its own normal flow and never hangs on katrix.
pub fn run(args: HookArgs) {
    let or_env = |v: Option<String>, key: &str| {
        v.or_else(|| std::env::var(key).ok()).unwrap_or_default()
    };
    let tool = or_env(args.tool, "KATRIX_TOOL");
    let agent_id = or_env(args.agent_id, "KATRIX_AGENT_ID");
    let endpoint = or_env(args.endpoint, "KATRIX_ENDPOINT");
    let token = or_env(args.token, "KATRIX_TOKEN");

    if endpoint.is_empty() {
        return; // not launched by katrix with a bridge — behave as a no-op
    }

    let payload = args.payload.unwrap_or_else(|| {
        let mut s = String::new();
        let _ = std::io::stdin().read_to_string(&mut s);
        s
    });
    let payload_json = if payload.trim().is_empty() {
        "null".to_string()
    } else {
        payload
    };

    let body = format!(
        "{{\"agentId\":{},\"tool\":{},\"event\":{},\"payload\":{}}}",
        quote(&agent_id),
        quote(&tool),
        quote(&args.event),
        payload_json
    );

    if let Some(decision) = post(&endpoint, &token, &body) {
        if !decision.is_empty() {
            print!("{decision}");
            let _ = std::io::stdout().flush();
        }
    }
}

/// POST the envelope to the bridge and return the response body (the decision).
/// The bridge holds the connection while the user decides, so the read blocks up
/// to ~595s (under the agent's own hook timeout).
fn post(endpoint: &str, token: &str, body: &str) -> Option<String> {
    let addr = endpoint.trim_start_matches("http://").trim_end_matches('/');
    let mut stream = TcpStream::connect(addr).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(595)));
    let req = format!(
        "POST / HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    stream.write_all(req.as_bytes()).ok()?;
    stream.flush().ok()?;
    let mut resp = Vec::new();
    stream.read_to_end(&mut resp).ok()?;
    let resp = String::from_utf8_lossy(&resp);
    resp.find("\r\n\r\n").map(|i| resp[i + 4..].to_string())
}

/// Minimal JSON string escaping (no serde — keeps the hot hook path std-only).
fn quote(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn hook_command_quotes_exe_and_carries_event() {
        let cmd = hook_command(&PathBuf::from("/opt/my app/katrix"), "PreToolUse");
        assert_eq!(cmd, "\"/opt/my app/katrix\" hook --event PreToolUse");
    }

    #[test]
    fn quote_escapes_control_chars() {
        assert_eq!(quote("a\"b\n"), "\"a\\\"b\\n\"");
    }
}
