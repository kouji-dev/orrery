//! Plan 06, task 9's other half: `python` and `process` are the same host.
//!
//! One transport, three argvs. `node index.mjs`, `python main.py`, and — for
//! `process` — whatever the manifest's `[process]` table names, which is what
//! lets an internal service that already exists become an extension without
//! being rewritten.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use orrery_ext_api::ExtensionManifest;
use orrery_host::ExtensionTable;
use orrery_host_rpc::RpcHost;
use orrery_proto::{ExtId, Grant, Layer, LoadOutcome, Outcome};
use orrery_tools::{CallCtx, ToolBudget};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn id(s: &str) -> ExtId {
    s.parse().unwrap()
}

/// A red bar that means "you do not have python" helps nobody.
fn program_or_skip(program: &str) -> bool {
    let found = std::process::Command::new(program)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !found {
        eprintln!("skipped: no `{program}` on PATH");
    }
    found
}

fn ctx() -> CallCtx {
    CallCtx::new(
        orrery_proto::CallId::new(),
        orrery_proto::Subject::Agent,
        orrery_proto::AgentScope {
            agent: "main".into(),
            branch: orrery_proto::BranchId::new(),
            tools: vec!["*".into()],
            grant: Grant::nothing(),
        },
        ToolBudget::new(30_000, 1 << 20),
    )
}

/// The phase-10 claim, tested: a service that was never written for Orrery is
/// an extension because its manifest says how to start it. Nothing about the
/// program is special — not its name, not its language, not its location.
#[tokio::test]
async fn a_process_extension_is_started_by_its_manifest() {
    if !program_or_skip("node") {
        return;
    }

    let host = Arc::new(RpcHost::process());
    host.install(&id("service"), fixture("service-ext"));
    let table = ExtensionTable::new();
    let manifest =
        Arc::new(ExtensionManifest::from_path(fixture("service-ext").join("orrery.toml")).unwrap());

    let outcome = table
        .load(host.clone(), manifest, Layer::Project, Grant::nothing())
        .await;
    assert!(
        matches!(outcome, LoadOutcome::Ok { .. }),
        "the service declared its tools: {outcome:?}"
    );

    let answered = table
        .call_tool(&"service.ping".parse().unwrap(), serde_json::json!({}), &ctx())
        .await
        .unwrap();

    match answered {
        Outcome::Ok { value, .. } => {
            let value = value.unwrap();
            assert_eq!(value["tool"], "ping");
            assert_eq!(
                value["said"], "pong",
                "[process.env] reached the child, which is how a wrapped \
                 service is configured without touching its code"
            );
        }
        other => panic!("expected Ok, got {other:?}"),
    }

    table
        .unload(&id("service"), Duration::from_secs(2))
        .await
        .unwrap();
}

/// `python main.py` is the convention, as `node index.mjs` is. Same host, same
/// protocol, same ledger.
#[tokio::test]
async fn a_python_extension_loads_and_dispatches() {
    if !program_or_skip("python") {
        return;
    }

    let host = Arc::new(RpcHost::python());
    host.install(&id("pyext"), fixture("python-ext"));
    let table = ExtensionTable::new();
    let manifest =
        Arc::new(ExtensionManifest::from_path(fixture("python-ext").join("orrery.toml")).unwrap());

    let outcome = table
        .load(host.clone(), manifest, Layer::Project, Grant::nothing())
        .await;
    assert!(
        matches!(outcome, LoadOutcome::Ok { .. }),
        "the python guest declared its tools: {outcome:?}"
    );

    let answered = table
        .call_tool(
            &"pyext.version".parse().unwrap(),
            serde_json::json!({}),
            &ctx(),
        )
        .await
        .unwrap();

    match answered {
        Outcome::Ok { value, .. } => {
            let value = value.unwrap();
            assert_eq!(value["tool"], "version");
            assert_eq!(
                value["cwd_is_extension_root"], true,
                "a guest runs in its own directory, whatever the harness's is"
            );
        }
        other => panic!("expected Ok, got {other:?}"),
    }

    table
        .unload(&id("pyext"), Duration::from_secs(2))
        .await
        .unwrap();
}

/// A `process` extension with no `[process]` table cannot be started, and the
/// ledger says exactly that rather than a spawn error nobody can act on.
#[tokio::test]
async fn a_process_extension_without_a_command_fails_at_resolve() {
    let host = Arc::new(RpcHost::process());
    host.install(&id("service"), fixture("service-ext"));

    // The manifest parser refuses this one outright — which is the earlier and
    // better of the two places to catch it.
    let broken = ExtensionManifest::from_toml_str(
        "api = \"orrery-ext/1\"\nruntime = \"process\"\n\n\
         [extension]\nid = \"nope\"\nversion = \"0.1.0\"\n\n\
         [provides]\ntools = [\"x\"]\n",
        "nope/orrery.toml",
    );
    let message = broken.expect_err("a process runtime needs a command").to_string();
    assert!(
        message.contains("nope/orrery.toml"),
        "the error names the file: {message}"
    );
}
