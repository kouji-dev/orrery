//! `orrery serve` + `orrery attach` — **the "no privileged client" proof**.
//!
//! One kernel, two renderers, neither of them in the process that owns the
//! kernel. Both reach it over the same named pipe, through the same handshake
//! and the same control RPC, and both render the same turn.

mod common;

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::{FINAL_TAIL, args, base, orrery};

/// A running `orrery serve`, and the endpoints it printed.
struct Served {
    child: Child,
    pipe: String,
    http: String,
    session: String,
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Start a kernel with no client attached and read its endpoints off stdout.
fn serve(dir: &std::path::Path, streams: &[&str]) -> Served {
    let mut args = base(dir, streams);
    args.push("serve".to_owned());
    let mut child = Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the orrery binary runs");
    let mut out = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let mut err = BufReader::new(child.stderr.take().expect("stderr is piped"));
    let mut pipe = String::new();
    let mut http = String::new();
    out.read_line(&mut pipe).expect("serve printed an endpoint");
    out.read_line(&mut http).expect("serve printed an endpoint");
    let pipe = pipe.trim().to_owned();
    let http = http.trim().to_owned();
    assert!(
        pipe.starts_with("pipe:"),
        "the first endpoint is the pipe: {pipe}"
    );
    assert!(
        http.starts_with("http://"),
        "the second endpoint is HTTP: {http}"
    );
    // **Scan, do not read one line.** stderr is narration, and `serve` may
    // narrate other things first — a workspace nobody has answered for is not
    // trusted, and saying so out loud is the point of that line. A test that
    // assumed the session was narration line 1 failed the moment anything else
    // had something to say, which is a test bug rather than a product one.
    let mut session = String::new();
    let mut line = String::new();
    while {
        line.clear();
        err.read_line(&mut line).expect("serve narrates") > 0
    } {
        if let Some(id) = line.trim().strip_prefix("orrery: session ") {
            session = id.to_owned();
            break;
        }
    }
    assert!(!session.is_empty(), "serve names the session on stderr");
    Served {
        child,
        pipe,
        http,
        session,
    }
}

/// Spawn an `orrery attach`, capturing its output.
fn attach(dir: &std::path::Path, extra: &[&str]) -> Child {
    let mut all = vec![
        "--workspace".to_owned(),
        dir.display().to_string(),
        "attach".to_owned(),
    ];
    all.extend(extra.iter().map(|s| (*s).to_owned()));
    Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(&all)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the orrery binary runs")
}

/// **The "no privileged client" proof.** Two renderers, one session, one turn,
/// and neither of them is the process holding the kernel.
///
/// The second renderer is the `json` client rather than Ink: Ink's only way to
/// start a turn is a keystroke in its composer, and AG-UI's HTTP transport has
/// no passive subscribe route, so a headless Ink cannot be made to render a
/// turn it did not itself start. See the amended Done-when bullet in
/// `17-cli.md`.
#[test]
fn two_renderers_one_session() {
    let dir = common::workspace();
    let served = serve(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);

    // Renderer one: the built-in TUI, drawing without a terminal.
    let tui = attach(dir.path(), &[&served.pipe, "--ui", "ratatui"]);
    // Renderer two: the json client, which also submits the turn.
    let json = attach(
        dir.path(),
        &[&served.pipe, "--ui", "json", "-p", "what is in Cargo.toml?"],
    );

    let json = json.wait_with_output().expect("the json client finishes");
    let tui = tui.wait_with_output().expect("the tui client finishes");

    let json_out = String::from_utf8_lossy(&json.stdout);
    assert!(
        json_out.contains("RUN_FINISHED"),
        "the json renderer saw the turn end: {json_out}"
    );
    assert!(json_out.contains(FINAL_TAIL), "…and its text: {json_out}");

    let tui_out = String::from_utf8_lossy(&tui.stdout);
    assert!(
        tui_out.contains("three crates"),
        "the ratatui renderer drew the same turn: {tui_out}"
    );
}

/// The session outlives the client that started the turn.
///
/// The submitting client is killed as soon as it has asked; re-attaching from
/// the start replays a turn that ran with nobody watching.
#[test]
fn outlives_a_client() {
    let dir = common::workspace();
    let served = serve(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);

    let mut first = attach(
        dir.path(),
        &[&served.pipe, "--ui", "json", "-p", "what is in Cargo.toml?"],
    );
    // Long enough for the submit to have been written, short enough that the
    // turn is still the kernel's problem and not this client's.
    std::thread::sleep(Duration::from_millis(50));
    let _ = first.kill();
    let _ = first.wait();

    // Re-attach from the beginning. `--since 0` is "everything after seq 0".
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let out = attach(dir.path(), &[&served.pipe, "--ui", "json", "--since", "0"])
            .wait_with_output()
            .expect("the re-attached client finishes");
        let text = String::from_utf8_lossy(&out.stdout).into_owned();
        if text.contains("RUN_FINISHED") {
            assert!(
                text.contains(FINAL_TAIL),
                "the replay carries what the turn said: {text}"
            );
            return;
        }
        assert!(
            Instant::now() < deadline,
            "the turn never completed without a client attached: {text}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The endpoint `serve` prints is one `attach` accepts and one
/// `ORRERY_ENDPOINT` carries — one string with a scheme prefix, and one parser.
///
/// **Both** endpoints, as of `GET /events`: the HTTP one used to be refused,
/// because AG-UI's transport was run-scoped and a passive attach would have sat
/// on a connection that never delivered anything. The transport now has a
/// passive subscribe route, so the only thing `attach` refuses is `inproc:`.
#[test]
fn the_endpoint_round_trips() {
    let dir = common::workspace();
    let served = serve(dir.path(), &["text-turn.jsonl"]);

    let out = orrery(&args(
        &["--workspace".to_owned(), dir.path().display().to_string()],
        &["attach", "inproc:", "--ui", "json"],
    ));
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("inproc:"),
        "and it says what to use instead: {stderr}"
    );
    drop(served);
}

/// `attach http://…` subscribes to a session it did not start.
///
/// This is the half plan 08's Done-when was missing: the HTTP listener grew a
/// `GET /events` route, so an SSE client — this one, Ink, any AG-UI client —
/// renders somebody else's turn instead of only the run it posted itself.
#[test]
fn attach_over_http_renders_the_turn() {
    let dir = common::workspace();
    let served = serve(dir.path(), &["tool-call.jsonl", "text-turn.jsonl"]);

    // The watcher: HTTP, passive, replaying from the start of the session.
    let watcher = attach(dir.path(), &[&served.http, "--ui", "json", "--since", "0"]);
    // The submitter: the pipe, because one of them has to ask.
    let submitter = attach(
        dir.path(),
        &[&served.pipe, "--ui", "json", "-p", "what is in Cargo.toml?"],
    );

    let submitter = submitter
        .wait_with_output()
        .expect("the submitter finishes");
    assert!(
        String::from_utf8_lossy(&submitter.stdout).contains("RUN_FINISHED"),
        "the pipe client saw the turn end"
    );
    let watcher = watcher.wait_with_output().expect("the watcher finishes");
    let seen = String::from_utf8_lossy(&watcher.stdout);
    assert!(
        seen.contains("RUN_FINISHED"),
        "the HTTP client saw the turn end: {seen}"
    );
    assert!(seen.contains(FINAL_TAIL), "…and what it said: {seen}");
}

/// `--ui ink` with no Node says so in a sentence rather than a spawn trace.
///
/// Node is usually *present* on a developer machine, so the assertion is on the
/// message the code produces, reached by pointing it at a bundle that is not
/// there — the other half of the same check, and the half that cannot be
/// arranged by deleting Node.
#[test]
fn ink_reports_what_is_missing() {
    let dir = common::workspace();
    let mut args = base(dir.path(), &["text-turn.jsonl"]);
    args.push("--ui".to_owned());
    args.push("ink".to_owned());
    let out = Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(&args)
        .env("ORRERY_INK_BUNDLE", dir.path().join("not-built.js"))
        .stdin(Stdio::null())
        .output()
        .expect("the orrery binary runs");

    assert_eq!(
        out.status.code(),
        Some(2),
        "a misconfiguration, not a crash"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not built") || stderr.contains("needs Node"),
        "it names what is missing: {stderr}"
    );
    assert!(
        stderr.contains("--ui ink") || stderr.contains("built-in TUI"),
        "…and what to do instead: {stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "and it is not a stack trace: {stderr}"
    );
}

/// The Ink client reaches the same kernel over the endpoint `serve` printed,
/// with nothing but `ORRERY_ENDPOINT` and `ORRERY_SESSION` to go on.
///
/// What is asserted is the **attach**, not the drawing: Ink puts stdin into raw
/// mode as it mounts, so a client spawned without a terminal refuses at the
/// render and not at the connection. Reaching that refusal means
/// `session.attach` was accepted — which is the half of the contract this
/// command owns. Skipped when Node or the built bundle is absent.
#[test]
fn the_ink_client_reaches_the_kernel() {
    let bundle =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../clients/ink/dist/index.js");
    if !bundle.exists() || Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipped: no node, or `pnpm -C harness/clients/ink build` has not been run");
        return;
    }

    let dir = common::workspace();
    let served = serve(dir.path(), &["text-turn.jsonl"]);
    let out = Command::new("node")
        .arg(&bundle)
        .env("ORRERY_ENDPOINT", &served.http)
        .env("ORRERY_SESSION", &served.session)
        .stdin(Stdio::null())
        .output()
        .expect("node runs");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("cannot reach the kernel"),
        "the Ink client could not attach: {stderr}"
    );
    assert!(
        stderr.is_empty() || stderr.contains("Raw mode"),
        "the only thing left in its way is the missing terminal: {stderr}"
    );
}
