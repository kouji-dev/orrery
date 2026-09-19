//! `orrery auth`, end to end through the binary, against an authorization
//! server that lives in this test.
//!
//! **No network request, and no key.** The server is a `TcpListener` on
//! `127.0.0.1` with an ephemeral port, started and stopped by the test; the
//! binary is pointed at it with `--auth-url`. Nothing leaves the machine and
//! nothing reaches Anthropic.
//!
//! What this file is for: the device-code flow had fourteen green tests and no
//! way in. `orrery auth login anthropic` — the command the binary's own
//! "sign in first" message tells people to run — answered `error:
//! unrecognized subcommand 'auth'`. A library test cannot catch that, so the
//! whole login is asserted here, from the outside, as a person does it.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::Output;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{args, home_with, orrery_in};

/// The code the person is shown, and the page they are shown it on.
const USER_CODE: &str = "WDJB-MJHT";
/// What the flow must print, and what it must poll.
const DEVICE_CODE: &str = "device-code-1";
/// What the server hands over once the person has finished.
const ACCESS_TOKEN: &str = "at-granted-1";

/// A fake authorization server: one device code, one `authorization_pending`,
/// then a grant.
///
/// The `authorization_pending` is not decoration. It is the normal case — a
/// person takes time — and a flow that only worked when the first poll
/// succeeded would be a flow that never worked.
struct FakeAuthServer {
    /// `http://127.0.0.1:<port>`, which is what `--auth-url` is given.
    base_url: String,
    polls: Arc<AtomicUsize>,
    /// Dropping this stops the thread after its next accept.
    _handle: std::thread::JoinHandle<()>,
}

impl FakeAuthServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().expect("the bound address").port();
        let base_url = format!("http://127.0.0.1:{port}");
        let polls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&polls);
        let handle = std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                if !serve(stream, &counter) {
                    break;
                }
            }
        });
        Self {
            base_url,
            polls,
            _handle: handle,
        }
    }

    fn polls(&self) -> usize {
        self.polls.load(Ordering::SeqCst)
    }
}

/// Answer one request. `false` when the listener should stop.
fn serve(mut stream: TcpStream, polls: &AtomicUsize) -> bool {
    let mut reader = BufReader::new(stream.try_clone().expect("the stream clones"));
    let mut request_line = String::new();
    if reader.read_line(&mut request_line).is_err() || request_line.is_empty() {
        return true;
    }
    let mut length = 0usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() {
            return true;
        }
        let trimmed = header.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
            .map(str::trim)
            .and_then(|v| v.parse::<usize>().ok())
        {
            length = value;
        }
    }
    let mut body = vec![0u8; length];
    if reader.read_exact(&mut body).is_err() {
        return true;
    }
    let body = String::from_utf8_lossy(&body).into_owned();

    let (status, payload) = if request_line.contains("/oauth/device/code") {
        (
            200,
            format!(
                "{{\"device_code\":\"{DEVICE_CODE}\",\"user_code\":\"{USER_CODE}\",\
                 \"verification_uri\":\"https://example.test/activate\",\
                 \"expires_in\":600,\"interval\":1}}"
            ),
        )
    } else if request_line.contains("/oauth/token") {
        assert!(
            body.contains(DEVICE_CODE),
            "the poll must carry the device code it was given: {body}"
        );
        let nth = polls.fetch_add(1, Ordering::SeqCst);
        if nth == 0 {
            (400, "{\"error\":\"authorization_pending\"}".to_owned())
        } else {
            (
                200,
                format!(
                    "{{\"access_token\":\"{ACCESS_TOKEN}\",\"refresh_token\":\"rt-1\",\
                     \"expires_in\":3600,\"account\":{{\"email_address\":\"a@example.test\"}}}}"
                ),
            )
        }
    } else {
        (404, "{}".to_owned())
    };

    let response = format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n\
         content-length: {}\r\nconnection: close\r\n\r\n{payload}",
        payload.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
    true
}

/// Everything after the verb: a workspace that is only a state directory.
fn base_args(dir: &Path) -> Vec<String> {
    vec![
        "--workspace".to_owned(),
        dir.display().to_string(),
        "--state-dir".to_owned(),
        dir.join(".orrery").display().to_string(),
    ]
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The whole path a person walks: not signed in, sign in, signed in, sign out.
#[test]
#[cfg_attr(
    not(feature = "anthropic"),
    ignore = "this build has no anthropic provider, so it has no anthropic login"
)]
fn login_then_status_then_logout() {
    let server = FakeAuthServer::start();
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");

    // 1 · Before anything, the binary says so — and exits 5, the code a turn
    //     that needs a login exits with.
    let before = orrery_in(home.path(), &args(&base_args(ws.path()), &["auth", "status"]));
    assert_eq!(
        before.status.code(),
        Some(5),
        "not signed in is exit 5: {}",
        stdout(&before)
    );
    assert!(
        stdout(&before).contains("not signed in"),
        "{}",
        stdout(&before)
    );

    // 2 · The login prints the code and the page, polls, and finishes.
    let login = orrery_in(
        home.path(),
        &args(
            &base_args(ws.path()),
            &["auth", "login", "anthropic", "--auth-url", &server.base_url],
        ),
    );
    let printed = stdout(&login);
    assert!(
        login.status.success(),
        "the login succeeds: {printed}{}",
        String::from_utf8_lossy(&login.stderr)
    );
    assert!(
        printed.contains(USER_CODE),
        "the person is shown the code to type: {printed}"
    );
    assert!(
        printed.contains("https://example.test/activate"),
        "and the page to type it on: {printed}"
    );
    assert!(
        printed.contains("signed in"),
        "and is told when it worked: {printed}"
    );
    assert!(
        server.polls() >= 2,
        "an `authorization_pending` is polled through, not given up on: {} poll(s)",
        server.polls()
    );

    // 3 · The token went to the `creds` grant under the state directory, and
    //     nowhere a person manages.
    let creds = ws.path().join(".orrery/credentials");
    let held = std::fs::read_to_string(&creds).expect("the grant was written");
    assert!(held.contains(ACCESS_TOKEN), "the access token is held");
    assert!(held.contains("rt-1"), "and the refresh token with it");

    // 4 · A **second process** sees it: that is the whole point of storing it.
    let after = orrery_in(
        home.path(),
        &args(&base_args(ws.path()), &["auth", "status", "--json"]),
    );
    assert!(after.status.success(), "signed in is exit 0");
    let state = common::jsonl(&after.stdout);
    assert_eq!(state[0]["state"], "ready", "{state:?}");
    assert_eq!(state[0]["grant"], "anthropic", "{state:?}");

    // 5 · And logging out takes all three parts back.
    let out = orrery_in(home.path(), &args(&base_args(ws.path()), &["auth", "logout"]));
    assert!(out.status.success(), "{}", stdout(&out));
    let held = std::fs::read_to_string(&creds).unwrap_or_default();
    assert!(
        !held.contains(ACCESS_TOKEN) && !held.contains("rt-1"),
        "a refresh token left behind is a credential the person believes they \
         revoked: {held}"
    );
    let last = orrery_in(home.path(), &args(&base_args(ws.path()), &["auth", "status"]));
    assert_eq!(last.status.code(), Some(5), "signed out again: {}", stdout(&last));
}

/// The verb exists in every build. Without the feature it says which flag is
/// missing, rather than `unrecognized subcommand`.
#[test]
fn the_verb_exists_whatever_this_build_has() {
    let home = home_with("");
    let ws = tempfile::tempdir().expect("a workspace");
    let out = orrery_in(home.path(), &args(&base_args(ws.path()), &["auth", "status"]));
    let said = format!("{}{}", stdout(&out), String::from_utf8_lossy(&out.stderr));
    assert!(
        !said.contains("unrecognized subcommand"),
        "the command the binary tells people to run must exist: {said}"
    );
}
