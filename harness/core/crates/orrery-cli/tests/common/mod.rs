//! A workspace, a fixture provider, and the binary under test.
//!
//! **No test in this crate makes a network request or needs a key.** The model
//! is one or more committed `.jsonl` streams replayed by
//! `orrery-ext-provider-fixture`, and everything else is a temporary directory.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// The committed model streams.
#[must_use]
pub fn stream(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../clients/conformance/streams")
        .join(name)
}

/// What the model asks to read in `tool-call.jsonl`.
pub const TARGET: &str = "Cargo.toml";

/// What that file says in the workspace these tests build.
pub const CONTENTS: &str = "[package]\nname = \"the-file-the-tool-really-read\"\n";

/// The assistant's last word in `text-turn.jsonl`.
pub const FINAL_TEXT: &str = "The workspace has three crates: proto, provider and kernel.";

/// The tail of it, which the stream sends as one delta. What to look for in a
/// frame-by-frame rendering, where the sentence arrives in two pieces.
pub const FINAL_TAIL: &str = "three crates: proto, provider and kernel.";

/// A temporary workspace with the file the fixture reads.
#[must_use]
pub fn workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary workspace");
    std::fs::write(dir.path().join(TARGET), CONTENTS).expect("the target file is written");
    dir
}

/// The global flags every test needs: where to work and what to replay.
#[must_use]
pub fn base(dir: &Path, streams: &[&str]) -> Vec<String> {
    let mut args = vec![
        "--workspace".to_owned(),
        dir.display().to_string(),
        "--state-dir".to_owned(),
        dir.join(".orrery").display().to_string(),
    ];
    for name in streams {
        args.push("--provider".to_owned());
        args.push(format!("fixture:{}", stream(name).display()));
    }
    args
}

/// The sandboxed home every run in this file's helpers gets.
///
/// One per test binary, made on first use and taken down with the process.
/// Without it these tests read the developer's own `~/.orrery` and the
/// machine's `%ProgramData%\Orrery\managed.toml`, which makes what they assert
/// a property of the box they ran on: a managed `deny` on a CI machine would
/// quietly change the answers. A test that wants a home with something *in* it
/// still builds its own with [`home_with`] and runs [`orrery_in`].
#[must_use]
pub fn sandbox_home() -> &'static Path {
    static HOME: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        let dir = tempfile::tempdir().expect("a temporary home");
        std::fs::create_dir_all(dir.path().join("ProgramData"))
            .expect("the sandboxed ProgramData");
        dir
    })
    .path()
}

/// Point a command's home, user profile and `%ProgramData%` inside the sandbox.
fn sandboxed(cmd: &mut Command) -> &mut Command {
    let home = sandbox_home();
    cmd.env("COLUMNS", "100")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ProgramData", home.join("ProgramData"))
}

/// Run the binary to completion, with a sandboxed home.
#[must_use]
pub fn orrery(args: &[String]) -> Output {
    sandboxed(Command::new(env!("CARGO_BIN_EXE_orrery")).args(args))
        .stdin(Stdio::null())
        .output()
        .expect("the orrery binary runs")
}

/// Run it with something on stdin, with a sandboxed home.
#[must_use]
pub fn orrery_with_stdin(args: &[String], stdin: &str) -> Output {
    use std::io::Write;
    let mut child = sandboxed(Command::new(env!("CARGO_BIN_EXE_orrery")).args(args))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the orrery binary runs");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(stdin.as_bytes())
        .expect("stdin is writable");
    child.wait_with_output().expect("the binary finishes")
}

/// Turn a string of args into the `Vec<String>` the helpers take.
#[must_use]
pub fn args(base: &[String], rest: &[&str]) -> Vec<String> {
    let mut all = base.to_vec();
    all.extend(rest.iter().map(|s| (*s).to_owned()));
    all
}

/// Every line of stdout, parsed as JSON. Panics with the offending line, which
/// is the only useful failure message for "stdout is not JSONL".
#[must_use]
pub fn jsonl(stdout: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("stdout is not JSONL: {e}\n  offending line: {line}"))
        })
        .collect()
}

/// The `type` field of every AG-UI frame on stdout, in order.
#[must_use]
pub fn event_types(stdout: &[u8]) -> Vec<String> {
    jsonl(stdout)
        .into_iter()
        // `Frame` flattens the event, so `type` is at the top level.
        .filter_map(|v| {
            v.get("type")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

/// Run the binary with a **sandboxed home**: `HOME`, `USERPROFILE` and
/// `ProgramData` all point inside `home`, so a test that resolves config layers
/// reads what it wrote and never the developer's own `~/.orrery`.
#[must_use]
pub fn orrery_in(home: &Path, args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(args)
        .env("COLUMNS", "100")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ProgramData", home.join("ProgramData"))
        .stdin(Stdio::null())
        .output()
        .expect("the orrery binary runs")
}

/// A home directory with a user config layer in it, and nothing else.
#[must_use]
pub fn home_with(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("a temporary home");
    std::fs::create_dir_all(dir.path().join(".orrery")).expect("the user directory");
    std::fs::write(dir.path().join(".orrery/config.toml"), config).expect("the user config");
    dir
}

/// Everything after the command in a sandboxed run: a workspace, and no
/// provider, because none of these commands starts a model.
#[must_use]
pub fn quiet(dir: &Path) -> Vec<String> {
    vec!["--workspace".to_owned(), dir.display().to_string()]
}
