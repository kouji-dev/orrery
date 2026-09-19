//! Node extensions, driven through the binary: installed, then called.
//!
//! # Why these are here and not in `orrery-host-rpc`
//!
//! `orrery-host-rpc` already loads `extensions/examples/node-hello` through the
//! real `RpcHost` and dispatches its tools — and it was green while
//! `orrery install ./node-hello` produced an extension that could not start.
//! The in-tree example resolved its SDK by a relative path that exists only in
//! this repository, nothing installed the SDK beside the copy, and the first
//! real turn answered:
//!
//! ```text
//! Failed { stage: Activate, message: "ERR_MODULE_NOT_FOUND: Cannot find module
//!   '<home>/.orrery/node/ext-sdk/src/index.mjs'" }
//! no-such-tool: there is no tool called `hello.greet` in this agent's tool set
//! ```
//!
//! A library test cannot see that, because the gap is between the installer and
//! the guest. So these drive the binary: install, then a turn.
//!
//! **No model, no network, no key.** The model is two `.jsonl` streams written
//! into the temporary workspace, replayed by the fixture provider.

mod common;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// A red bar that means "you do not have node" helps nobody.
fn node_or_skip() -> bool {
    let found = Command::new("node")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !found {
        eprintln!("skipped: no `node` on PATH");
    }
    found
}

/// `harness/extensions/examples/node-hello`, from this crate's directory.
fn worked_example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../extensions/examples/node-hello")
}

/// The binary, in a workspace, with a home of this test's own.
fn orrery(home: &Path, work: &Path, argv: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_orrery"))
        .args(argv)
        .current_dir(work)
        .env("COLUMNS", "100")
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ProgramData", home.join("ProgramData"))
        .stdin(Stdio::null())
        .output()
        .expect("the orrery binary runs")
}

fn own(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).to_owned()).collect()
}

/// Copy a directory's files (one level: these examples have no subdirectories).
fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("the destination");
    for entry in std::fs::read_dir(from).expect("the source") {
        let entry = entry.expect("an entry");
        if entry.file_type().expect("a file type").is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).expect("a copy");
        }
    }
}

/// A fixture pass that calls one tool with one JSON object.
fn calls(id: &str, call: &str, tool: &str, input: &str) -> String {
    let input = input.replace('"', "\\\"");
    format!(
        "{{\"t\":\"started\",\"id\":\"{id}\"}}\n\
         {{\"t\":\"tool-use-start\",\"call\":\"{call}\",\"name\":\"{tool}\"}}\n\
         {{\"t\":\"tool-use-delta\",\"call\":\"{call}\",\"json_fragment\":\"{input}\"}}\n\
         {{\"t\":\"tool-use-end\",\"call\":\"{call}\"}}\n\
         {{\"t\":\"done\",\"stop\":\"tool-use\"}}\n"
    )
}

/// A fixture pass that answers in prose and ends the turn.
fn says(id: &str, text: &str) -> String {
    format!(
        "{{\"t\":\"started\",\"id\":\"{id}\"}}\n\
         {{\"t\":\"text-delta\",\"text\":\"{text}\"}}\n\
         {{\"t\":\"done\",\"stop\":\"end-turn\"}}\n"
    )
}

/// Write a pass into the workspace and give back the `--provider` argument.
fn pass(work: &Path, name: &str, body: &str) -> Vec<String> {
    let path = work.join(name);
    std::fs::write(&path, body).expect("the stream is writable");
    own(&["--provider", &format!("fixture:{}", path.display())])
}

/// **Section 8, phase 10.** `orrery install ./node-hello --to user` and then a
/// real turn that calls `hello.greet`.
///
/// The install used to succeed and `ext list` used to say `ok` while the turn
/// could not start the guest at all.
#[test]
fn the_node_worked_example_installs_and_a_turn_calls_it() {
    if !node_or_skip() {
        return;
    }
    let home = tempfile::tempdir().expect("a sandboxed home");
    let work = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir_all(home.path().join("ProgramData")).expect("a sandboxed ProgramData");
    copy_dir(&worked_example(), &work.path().join("node-hello"));

    let installed = orrery(
        home.path(),
        work.path(),
        &own(&["install", "./node-hello", "--to", "user", "--yes"]),
    );
    assert!(
        installed.status.success(),
        "install: {}",
        String::from_utf8_lossy(&installed.stderr)
    );

    // The listing must agree with the run path, which is the other half of this
    // round: an extension that cannot start is not `ok`.
    let listed = orrery(home.path(), work.path(), &own(&["ext", "list"]));
    let listing = String::from_utf8_lossy(&listed.stdout);
    let line = listing
        .lines()
        .find(|l| l.starts_with("hello  "))
        .unwrap_or_else(|| panic!("the example is not listed: {listing}"));
    assert!(line.contains("ok"), "it loads: {listing}");

    // And a turn really calls it.
    let mut argv = own(&[
        "--workspace",
        &work.path().display().to_string(),
        "--state-dir",
        &work.path().join(".orrery").display().to_string(),
    ]);
    argv.extend(pass(
        work.path(),
        "one.jsonl",
        &calls(
            "msg_hello_1",
            "0192f3a0-0000-7000-8000-0000000000a1",
            "hello.greet",
            "{\"name\":\"world\"}",
        ),
    ));
    argv.extend(pass(
        work.path(),
        "two.jsonl",
        &says("msg_hello_2", "greeted"),
    ));
    argv.extend(own(&["run", "-p", "say hello", "--json"]));

    let out = orrery(home.path(), work.path(), &argv);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("ERR_MODULE_NOT_FOUND"),
        "the installed extension could not resolve its SDK: {stderr}"
    );
    assert!(
        !stdout.contains("no-such-tool") && !stderr.contains("no-such-tool"),
        "the tool never reached the model's tool set: {stdout}{stderr}"
    );
    assert!(
        stdout.contains("hello, world"),
        "the tool ran and its answer came back: {stdout}\n{stderr}"
    );
    assert_eq!(out.status.code(), Some(0), "{stderr}");
}

/// **Section 8, phase 2.** Two extensions claiming `search` both load, and
/// killing one leaves the session alive.
///
/// Driven through the binary because that is the only place the claim means
/// anything: the acceptance run's evidence for this was two extensions that
/// appeared in `ext list` and never actually loaded into a turn.
///
/// `alpha.search` answers and then exits its process; `beta.search` is called
/// afterwards, in the same session, and still answers.
#[test]
fn two_extensions_claim_search_and_killing_one_leaves_the_session_alive() {
    if !node_or_skip() {
        return;
    }
    let home = tempfile::tempdir().expect("a sandboxed home");
    let work = tempfile::tempdir().expect("a workspace");
    std::fs::create_dir_all(home.path().join("ProgramData")).expect("a sandboxed ProgramData");

    // Two extensions, same tool name, one of which takes itself down after
    // answering. Namespacing is what makes both reachable: `alpha.search` and
    // `beta.search` are different tools to the model and to the registry.
    for (id, after) in [
        ("alpha", "setTimeout(() => process.exit(1), 0);"),
        ("beta", ""),
    ] {
        let dir = work.path().join(id);
        std::fs::create_dir_all(&dir).expect("the extension directory");
        std::fs::write(
            dir.join("orrery.toml"),
            format!(
                "api = \"orrery-ext/1\"\nruntime = \"node\"\n\n\
                 [extension]\nid = \"{id}\"\nversion = \"0.1.0\"\n\n\
                 [provides]\ntools = [\"search\"]\n"
            ),
        )
        .expect("the manifest");
        std::fs::write(
            dir.join("index.mjs"),
            format!(
                "import {{ defineExtension }} from \"@orrery/ext\";\n\
                 export default defineExtension({{\n\
                 \x20 tools: {{\n\
                 \x20   search: {{\n\
                 \x20     description: \"Find something.\",\n\
                 \x20     input: {{ type: \"object\", properties: {{}} }},\n\
                 \x20     async run(_input, ctx) {{\n\
                 \x20       const out = ctx.ui.text(\"{id} found it\");\n\
                 \x20       {after}\n\
                 \x20       return out;\n\
                 \x20     }},\n\
                 \x20   }},\n\
                 \x20 }},\n\
                 }});\n"
            ),
        )
        .expect("the entry point");

        let installed = orrery(
            home.path(),
            work.path(),
            &own(&["install", &format!("./{id}"), "--to", "user", "--yes"]),
        );
        assert!(
            installed.status.success(),
            "install {id}: {}",
            String::from_utf8_lossy(&installed.stderr)
        );
    }

    // Both load, and the listing says so rather than merely naming them.
    let listed = orrery(home.path(), work.path(), &own(&["ext", "list"]));
    let listing = String::from_utf8_lossy(&listed.stdout);
    for id in ["alpha", "beta"] {
        let line = listing
            .lines()
            .find(|l| l.starts_with(&format!("{id}  ")))
            .unwrap_or_else(|| panic!("`{id}` is not listed: {listing}"));
        assert!(line.contains("ok"), "`{id}` did not load: {listing}");
        assert!(line.contains("search"), "`{id}` lost its tool: {listing}");
    }

    let mut argv = own(&[
        "--workspace",
        &work.path().display().to_string(),
        "--state-dir",
        &work.path().join(".orrery").display().to_string(),
    ]);
    argv.extend(pass(
        work.path(),
        "one.jsonl",
        &calls(
            "msg_search_1",
            "0192f3a0-0000-7000-8000-0000000000b1",
            "alpha.search",
            "{}",
        ),
    ));
    argv.extend(pass(
        work.path(),
        "two.jsonl",
        &calls(
            "msg_search_2",
            "0192f3a0-0000-7000-8000-0000000000b2",
            "beta.search",
            "{}",
        ),
    ));
    argv.extend(pass(
        work.path(),
        "three.jsonl",
        &says("msg_search_3", "both answered"),
    ));
    argv.extend(own(&["run", "-p", "search twice", "--json"]));

    let out = orrery(home.path(), work.path(), &argv);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stdout.contains("alpha found it"),
        "the first extension answered: {stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("beta found it"),
        "…and the second still answered after the first died: {stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("both answered"),
        "…and the session finished the turn: {stdout}\n{stderr}"
    );
    assert_eq!(out.status.code(), Some(0), "{stderr}");
}
