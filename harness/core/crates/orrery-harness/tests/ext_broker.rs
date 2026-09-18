//! Plan 06, the second gap wave 3 disclosed: the facade an extension holds is
//! per **call**, not per session.
//!
//! `ExtensionTable` used to hold one `Arc<dyn BrokerFacade>` for every call it
//! served, so a token minted while a tool ran was tied to a fresh `CallId` that
//! nothing could name afterwards — and `TokenLedger::revoke_call`, which is how
//! the kernel takes a cancelled call's capabilities back, could not reach it.
//!
//! These tests run the real thing: the real policy engine, the real
//! `LocalBroker`, `PolicyBroker` as the table's broker source, a native
//! extension dispatched through `ExtensionTable::call_tool`. Nothing here is a
//! rig standing in for production wiring.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use orrery_broker::LocalBroker;
use orrery_ext_api::{
    CallCtx as ExtCallCtx, HostError, NativeExtension, ReadRequest, ToolDef, WriteRequest,
};
use orrery_harness::{DEFAULT_RULES, LedgerRevoker, PolicyBroker};
use orrery_host::{ExtensionTable, NativeHost, NativeRegistry};
use orrery_kernel::CallRevoker;
use orrery_policy::{PolicyBuilder, PolicyEngine};
use orrery_proto::{
    AgentScope, Aspect, BranchId, CallId, Capability, Consent, ExtId, Grant, Layer, Outcome,
    Subject,
};
use orrery_tools::{CallCtx, ToolBudget};
use tokio::sync::oneshot;

/// The manifest of the extension below, in the shape every first-party bundle
/// uses — parsed by the one parser a third party's goes through.
const MANIFEST: &str = r#"
api = "orrery-ext/1"
runtime = "native"

[extension]
id = "probe"
version = "0.0.0"

[provides]
tools = ["two_reads", "late_write"]

[requires]
read = ["$WORKSPACE/**"]
write = ["$WORKSPACE/**"]
"#;

/// A tool that pauses in the middle, so a test can do something to it while it
/// is genuinely in flight.
struct Probe {
    /// Fired once the tool's first broker call has gone through.
    entered: parking_lot::Mutex<Option<oneshot::Sender<()>>>,
    /// Awaited before the tool's second broker call.
    resume: parking_lot::Mutex<Option<oneshot::Receiver<()>>>,
}

impl Probe {
    fn new() -> (Arc<Self>, oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (entered_tx, entered_rx) = oneshot::channel();
        let (resume_tx, resume_rx) = oneshot::channel();
        (
            Arc::new(Self {
                entered: parking_lot::Mutex::new(Some(entered_tx)),
                resume: parking_lot::Mutex::new(Some(resume_rx)),
            }),
            entered_rx,
            resume_tx,
        )
    }

    fn entered(&self) {
        if let Some(tx) = self.entered.lock().take() {
            let _ = tx.send(());
        }
    }

    async fn wait(&self) {
        let rx = self.resume.lock().take();
        if let Some(rx) = rx {
            let _ = rx.await;
        }
    }
}

#[async_trait]
impl NativeExtension for Probe {
    fn manifest(&self) -> &str {
        MANIFEST
    }

    fn manifest_path(&self) -> &str {
        "core/crates/orrery-harness/tests/ext_broker.rs"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef::new("two_reads")
                .described("Read, pause, read again.")
                .requiring([Aspect::Read]),
            ToolDef::new("late_write")
                .described("Pause, then write.")
                .atomic(true)
                .requiring([Aspect::Write]),
        ]
    }

    async fn call(
        &self,
        tool: &str,
        _input: serde_json::Value,
        ctx: &ExtCallCtx,
    ) -> Result<Outcome, HostError> {
        match tool {
            "two_reads" => {
                let first = ctx.broker.read(ReadRequest::new("a.txt", 4096)).await;
                self.entered();
                self.wait().await;
                let second = ctx.broker.read(ReadRequest::new("a.txt", 4096)).await;
                Ok(Outcome::Ok {
                    surface: None,
                    value: Some(serde_json::json!({
                        "first": first.is_ok(),
                        "second": second.is_ok(),
                        "second_error": second.err().map(|e| e.to_string()),
                    })),
                })
            }
            "late_write" => {
                self.entered();
                self.wait().await;
                let wrote = ctx
                    .broker
                    .write(WriteRequest::new("a.txt", b"clobbered".to_vec()))
                    .await;
                Ok(Outcome::Ok {
                    surface: None,
                    value: Some(serde_json::json!({ "wrote": wrote.is_ok() })),
                })
            }
            other => Err(HostError::NoSuchTool {
                ext: ctx.ext.clone(),
                tool: other.to_owned(),
            }),
        }
    }
}

/// A workspace, a real engine, a real broker, a real table.
struct Rig {
    dir: tempfile::TempDir,
    engine: Arc<PolicyEngine>,
    table: Arc<ExtensionTable>,
}

impl Rig {
    async fn open(probe: Arc<Probe>) -> Self {
        let dir = tempfile::tempdir().expect("a temporary workspace");
        let rules = PolicyBuilder::new(dir.path())
            .layer_toml(DEFAULT_RULES, "orrery.toml", Layer::Project, true)
            .expect("the default rules parse")
            .build()
            .expect("the default rules compile");
        let engine = Arc::new(PolicyEngine::new(rules));
        let local = Arc::new(LocalBroker::new(engine.ledger().clone()));
        let grant = Grant {
            capabilities: vec![
                Capability::all(Aspect::Tool),
                Capability::all(Aspect::Read),
                Capability::all(Aspect::Write),
            ],
            consent: Consent::Always,
        };
        let scope = AgentScope {
            agent: "main".to_owned(),
            branch: BranchId::new(),
            tools: vec!["*".to_owned()],
            grant: grant.clone(),
        };
        let budget = ToolBudget::new(30_000, 1 << 20);
        let facade = PolicyBroker::new(
            engine.clone(),
            local,
            dir.path(),
            Subject::Agent,
            scope,
            budget,
        );

        // The production wiring, exactly as `build.rs` does it.
        let table = ExtensionTable::with_broker_source(facade);
        let mut natives = NativeRegistry::new();
        natives.register(probe);
        let host = Arc::new(NativeHost::new(natives));
        let ext: ExtId = "probe".parse().expect("`probe` is an ext id");
        let manifest = host.manifest_of(&ext).expect("the probe is registered");
        let loaded = table.load(host, manifest, Layer::Project, grant).await;
        assert!(
            matches!(loaded, orrery_proto::LoadOutcome::Ok { .. }),
            "the probe loads: {loaded:?}"
        );

        Self { dir, engine, table }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn ctx(&self, call: CallId) -> CallCtx {
        CallCtx::new(
            call,
            Subject::Agent,
            AgentScope {
                agent: "main".to_owned(),
                branch: BranchId::new(),
                tools: vec!["*".to_owned()],
                grant: Grant::nothing(),
            },
            ToolBudget::new(30_000, 1 << 20),
        )
    }
}

fn value_of(outcome: &Outcome) -> serde_json::Value {
    match outcome {
        Outcome::Ok { value, .. } => value.clone().unwrap_or(serde_json::Value::Null),
        other => panic!("expected Ok, got {other:?}"),
    }
}

/// The one that names the gap: the kernel revokes a call *that is running*, and
/// the tool's next broker call is refused.
///
/// It can only pass if the facade the tool holds was built `for_call` with the
/// dispatch's own `CallId` — which is what `ExtensionTable` now does per
/// dispatch, and did not before.
#[tokio::test]
async fn revoke_call_reaches_an_in_flight_tool_call() {
    let (probe, entered, resume) = Probe::new();
    let rig = Rig::open(probe).await;
    std::fs::write(rig.path("a.txt"), "hello\n").unwrap();

    let call = CallId::new();
    let table = rig.table.clone();
    let ctx = rig.ctx(call);
    let running = tokio::spawn(async move {
        table
            .call_tool(
                &"probe.two_reads".parse().unwrap(),
                serde_json::json!({}),
                &ctx,
            )
            .await
            .expect("the harness carried the call")
    });

    entered.await.expect("the tool reached its first read");

    // Exactly what the kernel does when a turn is cancelled.
    LedgerRevoker::new(rig.engine.ledger().clone()).revoke(call);
    let _ = resume.send(());

    let outcome = running.await.expect("the call settles");
    let value = value_of(&outcome);
    assert_eq!(value["first"], true, "the first read was allowed: {value}");
    assert_eq!(
        value["second"], false,
        "a revoked call's next broker call must be refused: {value}"
    );
}

/// The other half of `for_call`: the cancel token. A cancelled call's write
/// never lands, whatever the tool does with it.
#[tokio::test]
async fn a_cancelled_call_cannot_write() {
    let (probe, entered, resume) = Probe::new();
    let rig = Rig::open(probe).await;
    std::fs::write(rig.path("a.txt"), "original\n").unwrap();

    let call = CallId::new();
    let table = rig.table.clone();
    let ctx = rig.ctx(call);
    let running = tokio::spawn(async move {
        table
            .call_tool(
                &"probe.late_write".parse().unwrap(),
                serde_json::json!({}),
                &ctx,
            )
            .await
            .expect("the harness carried the call")
    });

    entered.await.expect("the tool is in flight");
    // The instance-wide token the table derives each call's from.
    rig.table
        .get(&"probe".parse().unwrap())
        .expect("loaded")
        .cancel_token()
        .cancel();
    let _ = resume.send(());

    let outcome = running.await.expect("the call settles");
    let value = value_of(&outcome);
    assert_eq!(
        value["wrote"], false,
        "a cancelled call's write must not commit: {value}"
    );
    assert_eq!(
        std::fs::read_to_string(rig.path("a.txt")).unwrap(),
        "original\n",
        "the file the cancelled write aimed at is untouched"
    );
}
