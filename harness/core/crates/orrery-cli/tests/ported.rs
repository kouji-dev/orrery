//! Phase 4 through the **binary**: three ported extensions, no drawing code.
//!
//! # Why this file exists
//!
//! §8 phase 4 asks for "three ported extensions [that] render with no drawing
//! code of their own". That was proven — thoroughly — inside `orrery-ported`'s
//! own tests, which load the three through `orrery-host` and encode the frames
//! by hand. It was **not** provable through the product: nothing in
//! `orrery-cli` or `orrery-harness` named the three crates, `orrery ext list`
//! never listed them, and no turn could call one. A criterion a library test
//! satisfies and the binary cannot exercise is the defect this round exists to
//! stop, not an exception to it.
//!
//! So the three are compiled in behind `example-extensions` — off by default,
//! because a shipped binary has no business carrying demos — and these tests
//! drive the built binary with the feature on.
//!
//!     cargo test -p orrery-cli --features example-extensions --test ported
//!
//! # Both clients, one turn
//!
//! `run --json` is the json client. `serve` + `attach --ui ratatui` is the
//! ratatui client drawing into a headless scrollback, which is the same code
//! path a terminal takes with the terminal swapped out. The extensions are
//! identical in both, and neither of them knows which one is reading.
//!
//! **No network, no key**: the model is a committed-shape `.jsonl` this file
//! writes into a temporary directory.
#![cfg(feature = "example-extensions")]

mod common;

use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Child, Command, Stdio};

/// What the model asks each of the three to describe.
///
/// The same inputs `orrery-ported`'s `examples()` uses, so the surfaces this
/// turn produces are the surfaces that crate snapshots — one description of
/// what a census, a review and a release train look like, not two.
fn stream() -> String {
    let census = r#"{"crates":[{"name":"orrery-proto","area":"core","published":true},{"name":"orrery-ext-api","area":"core","published":true},{"name":"orrery-surface","area":"core","published":false},{"name":"patch-review","area":"extension","published":false}]}"#;
    let review = r#"{"path":"harness/core/crates/orrery-ext-api/src/sink.rs","hunks":[{"old_start":28,"old_lines":3,"new_start":28,"new_lines":3,"lines":[{"op":"context","text":"use orrery_proto::Surface;"},{"op":"del","text":"use orrery_ext_api::SurfaceSink;"},{"op":"add","text":"use crate::ctx::SurfaceSink;"}]}]}"#;
    let train = r#"{"release":"v0.25.0","stages":[{"id":"schema","label":"surface schema","status":"done"},{"id":"differ","label":"kernel differ","status":"done"},{"id":"ratatui","label":"ratatui client","status":"running"},{"id":"ink","label":"ink client","status":"pending"},{"id":"ade","label":"ade client","status":"pending"}]}"#;

    let mut out = String::from("{\"t\":\"started\",\"id\":\"msg_ported\"}\n");
    out.push_str("{\"t\":\"text-delta\",\"text\":\"Three ported extensions, described not drawn.\"}\n");
    for (n, (tool, input)) in [
        ("example-workspace-census.census", census),
        ("example-patch-review.review", review),
        ("example-release-train.status", train),
    ]
    .into_iter()
    .enumerate()
    {
        let call = format!("0192f3a0-0000-7000-8000-0000000000a{}", n + 1);
        out.push_str(&format!(
            "{{\"t\":\"tool-use-start\",\"call\":\"{call}\",\"name\":\"{tool}\"}}\n"
        ));
        out.push_str(&format!(
            "{{\"t\":\"tool-use-delta\",\"call\":\"{call}\",\"json_fragment\":{}}}\n",
            serde_json::Value::String(input.to_owned())
        ));
        out.push_str(&format!(
            "{{\"t\":\"tool-use-end\",\"call\":\"{call}\"}}\n"
        ));
    }
    out.push_str(
        "{\"t\":\"usage\",\"usage\":{\"input_tokens\":50,\"output_tokens\":20,\"cache_hits\":0}}\n",
    );
    out.push_str("{\"t\":\"done\",\"stop\":\"tool-use\"}\n");
    out
}

/// A workspace with the turn's model stream beside it.
fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary workspace");
    std::fs::write(dir.path().join("ported.jsonl"), stream()).expect("the model stream");
    dir
}

/// The global flags: this workspace, its own state, and the two passes — the
/// tool call, then a plain text answer so the turn ends.
fn base(dir: &Path) -> Vec<String> {
    vec![
        "--workspace".to_owned(),
        dir.path_str(),
        "--state-dir".to_owned(),
        dir.join(".orrery").display().to_string(),
        "--provider".to_owned(),
        format!("fixture:{}", dir.join("ported.jsonl").display()),
        "--provider".to_owned(),
        format!("fixture:{}", common::stream("text-turn.jsonl").display()),
    ]
}

/// `Path::display().to_string()`, spelled once.
trait PathStr {
    fn path_str(&self) -> String;
}

impl PathStr for Path {
    fn path_str(&self) -> String {
        self.display().to_string()
    }
}

/// The criterion's first half: the binary can see them at all.
///
/// `orrery ext list` reads `features::register_native` — the same registration
/// the run path uses — so a build that lists them is a build whose turns can
/// call them. That is the whole reason they are compiled in rather than
/// discovered on disk: `features::host_for` answers `None` for `native`, and
/// that answer is correct.
#[test]
fn ext_list_names_the_three() {
    let dir = workspace();
    let out = common::orrery(&common::args(
        &["--workspace".to_owned(), dir.path().path_str()],
        &["ext", "list"],
    ));
    let stdout = String::from_utf8_lossy(&out.stdout);
    for ext in [
        "example-workspace-census",
        "example-patch-review",
        "example-release-train",
    ] {
        assert!(stdout.contains(ext), "`{ext}` is not listed: {stdout}");
    }
}

/// The criterion itself, in the json client: one turn, three extensions, three
/// surfaces the kernel's own differ produced.
#[test]
fn a_real_turn_renders_all_three_in_json() {
    let dir = workspace();
    let out = common::orrery(&common::args(
        &base(dir.path()),
        &["--json", "run", "-p", "show me the three"],
    ));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("RUN_FINISHED"),
        "the turn ran: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    // Each extension's own words, from its own surface. Not the tool's return
    // value — the **described** surface, which is the thing phase 4 is about.
    assert!(
        stdout.contains("of them published"),
        "the census rendered: {stdout}"
    );
    assert!(
        stdout.contains("changed lines to sink.rs"),
        "the review rendered, question and all: {stdout}"
    );
    assert!(
        stdout.contains("example-release-train.timeline"),
        "…and the release train's own custom surface kind: {stdout}"
    );
}

/// The same turn, drawn by the ratatui client, which is a different process
/// from the one holding the kernel.
///
/// Neither extension contains a `print!`, a width or a terminal crate, and
/// neither learns which of these two tests it is being read in.
#[test]
fn the_same_turn_renders_in_ratatui() {
    let dir = workspace();
    let mut args = base(dir.path());
    args.push("serve".to_owned());
    let mut served = Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the orrery binary runs");
    let mut out = BufReader::new(served.stdout.take().expect("stdout is piped"));
    let mut pipe = String::new();
    out.read_line(&mut pipe).expect("serve printed an endpoint");
    let pipe = pipe.trim().to_owned();
    assert!(pipe.starts_with("pipe:"), "the first endpoint is the pipe");

    let tui = attach(dir.path(), &[&pipe, "--ui", "ratatui"]);
    let submitted = attach(
        dir.path(),
        &[&pipe, "--ui", "json", "-p", "show me the three"],
    )
    .wait_with_output()
    .expect("the json client finishes");
    assert!(
        String::from_utf8_lossy(&submitted.stdout).contains("RUN_FINISHED"),
        "the turn ran"
    );

    let drawn = tui.wait_with_output().expect("the tui client finishes");
    let _ = served.kill();
    let _ = served.wait();
    let text = String::from_utf8_lossy(&drawn.stdout);

    // The table the census described, laid out by the client.
    assert!(
        text.contains("orrery-ext-api") && text.contains("published"),
        "the census table is drawn: {text}"
    );
    // The diff and its question, including the choices.
    assert!(
        text.contains("+use crate::ctx::SurfaceSink;") && text.contains("Apply"),
        "the patch review is drawn: {text}"
    );
    // The task list, the progress bar and the custom surface's fallback.
    assert!(
        text.contains("surface schema") && text.contains("v0.25.0"),
        "the release train is drawn: {text}"
    );
}

/// Spawn an `orrery attach`, capturing its output.
fn attach(dir: &Path, extra: &[&str]) -> Child {
    let mut all = vec![
        "--workspace".to_owned(),
        dir.path_str(),
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
