//! The hook-client side: what an agent's pre-tool / status hook actually runs.
//! It forwards the tool request to katrix's loopback bridge and prints whatever
//! decision JSON the bridge hands back (which the agent then obeys). Uses only
//! std so it stays fast and never blocks the agent on katrix internals — no
//! output (or no bridge) means the agent just proceeds with its own flow.
//!
//! Shipped as a hidden subcommand of the main binary (`katrix __hook <EVENT>`)
//! rather than a separate sidecar, so it is always co-located with the app and
//! needs no bundling step. Bridge details arrive via env stamped on the agent
//! process; the hook event name is the subcommand argument.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

/// The shell command an agent runs for one hook event — the app exe re-invoked
/// with the hook subcommand and the event. Quoted so a path with spaces is safe.
pub fn hook_command(exe: &Path, event: &str) -> String {
    format!("\"{}\" {} {}", exe.display(), crate::HOOK_SUBCOMMAND, event)
}

/// Run the hook client for `event`: forward stdin + env to the bridge, print the
/// decision. Called from `main` when launched as `katrix __hook <EVENT>`.
pub fn run(event: String) {
    let mut payload = String::new();
    let _ = std::io::stdin().read_to_string(&mut payload);

    let agent_id = std::env::var("KATRIX_AGENT_ID").unwrap_or_default();
    let tool = std::env::var("KATRIX_TOOL").unwrap_or_default();
    let endpoint = std::env::var("KATRIX_ENDPOINT").unwrap_or_default();
    let token = std::env::var("KATRIX_TOKEN").unwrap_or_default();

    // no bridge configured → say nothing, let the agent behave normally
    if endpoint.is_empty() {
        return;
    }

    let payload_json = if payload.trim().is_empty() {
        "null".to_string()
    } else {
        payload
    };
    let body = format!(
        "{{\"agentId\":{},\"tool\":{},\"event\":{},\"payload\":{}}}",
        quote(&agent_id),
        quote(&tool),
        quote(&event),
        payload_json
    );

    let addr = endpoint
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string();
    let Ok(mut stream) = TcpStream::connect(&addr) else {
        return;
    };
    // generous read timeout: the bridge holds the connection while the user
    // decides (the agent's own hook timeout is the real ceiling).
    let _ = stream.set_read_timeout(Some(Duration::from_secs(595)));

    let req = format!(
        "POST / HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.as_bytes().len()
    );
    if stream.write_all(req.as_bytes()).is_err() {
        return;
    }
    let _ = stream.flush();

    let mut resp = Vec::new();
    if stream.read_to_end(&mut resp).is_err() {
        return;
    }
    let resp = String::from_utf8_lossy(&resp);
    if let Some(idx) = resp.find("\r\n\r\n") {
        let out = &resp[idx + 4..];
        if !out.is_empty() {
            print!("{out}");
            let _ = std::io::stdout().flush();
        }
    }
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
        assert_eq!(cmd, "\"/opt/my app/katrix\" __hook PreToolUse");
    }

    #[test]
    fn quote_escapes_control_chars() {
        assert_eq!(quote("a\"b\n"), "\"a\\\"b\\n\"");
    }
}
