//! Plan 06, task 9: `@orrery/ext` against the real host.
//!
//! The SDK cannot establish by itself that it speaks the protocol — its own
//! tests are written against its own idea of the wire. This one loads
//! `extensions/examples/node-hello`, which is built on the SDK, through the
//! `RpcHost` a session uses, and dispatches its tools. If the two halves ever
//! disagree, this is where it shows.
//!
//! The hand-written `echo-ext` fixture proves the *other* property: that the
//! protocol is implementable from the spec with no SDK at all.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use orrery_ext_api::{
    BrokerFacade, BrokerResult, ExtensionManifest, ListEntry, ListRequest, Listing,
};
use orrery_host::ExtensionTable;
use orrery_host_rpc::RpcHost;
use orrery_proto::{
    AgentScope, Aspect, BranchId, Capability, Consent, ExtId, Grant, Layer, LoadOutcome, Outcome,
};
use orrery_tools::{CallCtx, ToolBudget};

/// `harness/extensions/examples/node-hello`, from this crate's directory.
fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../extensions/examples/node-hello")
}

fn id(s: &str) -> ExtId {
    s.parse().unwrap()
}

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

/// A broker that answers exactly one thing, so the test is about the transport
/// and not about the filesystem.
#[derive(Debug)]
struct ListsTwoFiles;

#[async_trait]
impl BrokerFacade for ListsTwoFiles {
    async fn list(&self, req: ListRequest) -> BrokerResult<Listing> {
        assert!(req.recursive, "the example asks for a recursive walk");
        Ok(Listing {
            entries: vec![
                ListEntry {
                    path: req.path.join("a.txt"),
                    is_dir: false,
                    size: Some(3),
                },
                ListEntry {
                    path: req.path.join("b.txt"),
                    is_dir: false,
                    size: Some(4),
                },
                ListEntry {
                    path: req.path.join("sub"),
                    is_dir: true,
                    size: None,
                },
            ],
            truncated: false,
        })
    }
}

fn grant_read() -> Grant {
    Grant {
        capabilities: vec![Capability::all(Aspect::Read)],
        consent: Consent::Always,
    }
}

fn ctx() -> CallCtx {
    CallCtx::new(
        orrery_proto::CallId::new(),
        orrery_proto::Subject::Agent,
        AgentScope {
            agent: "main".into(),
            branch: BranchId::new(),
            tools: vec!["*".into()],
            grant: Grant::nothing(),
        },
        ToolBudget::new(30_000, 1 << 20),
    )
}

async fn load(grant: Grant) -> (Arc<ExtensionTable>, Arc<RpcHost>, LoadOutcome) {
    let host = Arc::new(RpcHost::node());
    host.install(&id("hello"), example());
    host.set_broker(Arc::new(ListsTwoFiles));
    let table = ExtensionTable::with_broker(Arc::new(ListsTwoFiles));
    let manifest =
        Arc::new(ExtensionManifest::from_path(example().join("orrery.toml")).expect("it parses"));
    let outcome = table.load(host.clone(), manifest, Layer::Project, grant).await;
    (table, host, outcome)
}

/// An extension written with the SDK loads, declares what the manifest promised,
/// and dispatches.
#[tokio::test]
async fn the_sdk_speaks_the_protocol() {
    if !node_or_skip() {
        return;
    }
    let (table, host, outcome) = load(grant_read()).await;
    assert!(
        matches!(outcome, LoadOutcome::Ok { .. }),
        "the SDK answered ext/load with both tools: {outcome:?} (stderr: {:?})",
        host.stderr_tail(&id("hello"))
    );

    let answered = table
        .call_tool(
            &"hello.greet".parse().unwrap(),
            serde_json::json!({ "name": "harness" }),
            &ctx(),
        )
        .await
        .unwrap();

    match answered {
        Outcome::Ok { surface, .. } => {
            let surface = surface.expect("`ctx.ui.text` describes a surface");
            let json = serde_json::to_value(&surface).unwrap();
            assert_eq!(json["kind"]["t"], "text");
            assert_eq!(json["kind"]["value"], "hello, harness");
        }
        other => panic!("expected Ok, got {other:?}"),
    }

    table
        .unload(&id("hello"), Duration::from_secs(2))
        .await
        .unwrap();
}

/// `ctx.fs.list` is a guest→broker request travelling *up* the connection its
/// `tool/call` came down, while that call is still in flight.
#[tokio::test]
async fn a_guest_reaches_the_broker_mid_call() {
    if !node_or_skip() {
        return;
    }
    let (table, _host, _) = load(grant_read()).await;

    let answered = table
        .call_tool(
            &"hello.count_files".parse().unwrap(),
            serde_json::json!({ "path": "." }),
            &ctx(),
        )
        .await
        .unwrap();

    match answered {
        Outcome::Ok { surface, .. } => {
            let json = serde_json::to_value(surface.expect("a table")).unwrap();
            assert_eq!(json["kind"]["t"], "table");
            assert_eq!(
                json["kind"]["rows"][0][1]["text"], "2",
                "two files and one directory came back; the directory is not a file"
            );
        }
        other => panic!("expected Ok, got {other:?}"),
    }

    table
        .unload(&id("hello"), Duration::from_secs(2))
        .await
        .unwrap();
}

/// A tool whose aspect is not granted is **disabled** — not offered to the
/// model, and refused with a reason if something asks for it anyway. The SDK
/// declares `requires` for exactly this.
#[tokio::test]
async fn an_ungranted_tool_is_disabled_not_broken() {
    if !node_or_skip() {
        return;
    }
    let (table, _host, outcome) = load(Grant::nothing()).await;
    assert!(
        matches!(outcome, LoadOutcome::Degraded { .. }),
        "no `read` grant, so the extension degrades: {outcome:?}"
    );

    let refused = table
        .call_tool(
            &"hello.count_files".parse().unwrap(),
            serde_json::json!({ "path": "." }),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(
        matches!(refused, Outcome::Denied { .. }),
        "the disabled tool is refused with a reason: {refused:?}"
    );

    // And the tool that needs nothing still works.
    let fine = table
        .call_tool(
            &"hello.greet".parse().unwrap(),
            serde_json::json!({ "name": "you" }),
            &ctx(),
        )
        .await
        .unwrap();
    assert!(fine.is_ok(), "the rest of the extension is untouched: {fine:?}");

    table
        .unload(&id("hello"), Duration::from_secs(2))
        .await
        .unwrap();
}
