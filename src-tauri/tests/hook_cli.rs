//! End-to-end test of the real `orrery hook` CLI subprocess: it must connect to
//! the bridge endpoint, POST the request (payload from stdin), and print back the
//! decision the bridge returns. Here a stand-in TCP server plays the bridge.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

#[test]
fn hook_cli_posts_request_and_prints_the_decision() {
    // stand-in bridge: accept one connection, read the request, reply allow.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).unwrap();
        let req = String::from_utf8_lossy(&buf[..n]).to_string();
        let body = r#"{"hookSpecificOutput":{"permissionDecision":"allow"}}"#;
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(resp.as_bytes()).unwrap();
        req // hand the captured request back to the test
    });

    let exe = env!("CARGO_BIN_EXE_orrery");
    let mut child = Command::new(exe)
        .args([
            "hook",
            "--event",
            "PreToolUse",
            "--tool",
            "claude",
            "--agent-id",
            "a1",
            "--endpoint",
            &format!("http://127.0.0.1:{port}"),
            "--token",
            "secret",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn orrery hook");

    // the agent delivers the tool request on stdin; close it so the CLI reads EOF
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(br#"{"tool_name":"Bash","tool_input":{"command":"ls"}}"#)
        .unwrap();
    drop(stdin);

    let out = child.wait_with_output().expect("hook exits");
    let req = server.join().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);

    // the CLI printed the bridge's decision verbatim → the agent would allow
    assert!(
        stdout.contains(r#""permissionDecision":"allow""#),
        "stdout should carry the decision, got: {stdout}"
    );
    // and it forwarded a well-formed envelope (auth + payload) to the bridge
    assert!(req.contains("Authorization: Bearer secret"), "auth header sent");
    assert!(req.contains(r#""agentId":"a1""#), "agent id in envelope");
    assert!(req.contains("ls"), "stdin payload forwarded");
}
