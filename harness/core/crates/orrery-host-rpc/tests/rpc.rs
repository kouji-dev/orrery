//! A child process is an extension like any other — until it dies.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use orrery_ext_api::ExtensionManifest;
use orrery_host::ExtensionTable;
use orrery_host_rpc::{Containment, RpcHost};
use orrery_proto::{ExtId, Grant, Layer, LoadOutcome, Outcome};
use orrery_tools::{CallCtx, ToolBudget};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/echo-ext")
}

fn id(s: &str) -> ExtId {
    s.parse().unwrap()
}

/// `node` is how this runtime is reached. Without it there is nothing to test,
/// and a red bar that means "you do not have node" helps nobody.
fn node_or_skip() -> bool {
    let found = std::process::Command::new("node")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !found {
        eprintln!("skipped: no `node` on PATH");
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

#[tokio::test]
async fn child_dies_with_us() {
    if !node_or_skip() {
        return;
    }

    // A child that would happily outlive us.
    let mut command = tokio::process::Command::new("node");
    command.args(["-e", "setTimeout(() => {}, 600000)"]);
    Containment::configure(&mut command);
    let mut child = command.spawn().expect("node starts");
    let containment = Containment::capture(&child).expect("the child is contained");
    let pid = child.id().expect("a live child has a pid");

    // Tearing down our handle on it is what a crash, a force-kill or a clean
    // exit all look like from the child's side.
    drop(containment);

    let gone = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(Some(status)) = child.try_wait() {
                return status;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await;

    assert!(
        gone.is_ok(),
        "pid {pid} outlived the containment that owned it"
    );
}

#[tokio::test]
async fn a_node_extension_loads_and_dispatches() {
    if !node_or_skip() {
        return;
    }

    let host = Arc::new(RpcHost::node());
    host.install(&id("echo"), fixture());
    let table = ExtensionTable::new();
    let manifest = Arc::new(ExtensionManifest::from_path(fixture().join("orrery.toml")).unwrap());

    let outcome = table
        .load(host.clone(), manifest, Layer::Project, Grant::nothing())
        .await;
    assert!(
        matches!(outcome, LoadOutcome::Ok { .. }),
        "the guest declared its tools: {outcome:?}"
    );

    let answered = table
        .call_tool(
            &"echo.say".parse().unwrap(),
            serde_json::json!({ "text": "hello" }),
            &ctx(),
        )
        .await
        .unwrap();

    match answered {
        Outcome::Ok { value, .. } => {
            assert_eq!(value.unwrap()["said"], "hello");
        }
        other => panic!("expected Ok, got {other:?}"),
    }

    table
        .unload(&id("echo"), Duration::from_secs(2))
        .await
        .unwrap();
}

#[tokio::test]
async fn crash_degrades_not_kills() {
    if !node_or_skip() {
        return;
    }

    let host = Arc::new(RpcHost::node());
    host.install(&id("echo"), fixture());
    let table = ExtensionTable::new();
    let manifest = Arc::new(ExtensionManifest::from_path(fixture().join("orrery.toml")).unwrap());
    table
        .load(host.clone(), manifest, Layer::Project, Grant::nothing())
        .await;

    // `boom` makes the child exit in the middle of the call.
    let settled = table
        .call_tool(&"echo.boom".parse().unwrap(), serde_json::json!({}), &ctx())
        .await
        .expect("a dead child is not a failed dispatch");

    assert!(
        matches!(settled, Outcome::Failed { .. }),
        "the call settles Failed: {settled:?}"
    );

    // The session lives: the instance is still there, marked Degraded, and the
    // next call gets a clear answer rather than a hang.
    let instance = table
        .get(&id("echo"))
        .expect("the extension is still known");
    assert_eq!(
        instance.state(),
        orrery_ext_api::InstanceState::Degraded,
        "the extension degrades; the session does not end"
    );

    let after = table
        .call_tool(
            &"echo.say".parse().unwrap(),
            serde_json::json!({ "text": "still here" }),
            &ctx(),
        )
        .await
        .expect("still no turn failure");
    assert!(
        matches!(after, Outcome::Failed { .. }),
        "a call to a crashed child fails, in a shape the model can re-plan \
         around: {after:?}"
    );
}
