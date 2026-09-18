//! A **real** broker behind the extension-facing facade.
//!
//! Every test in this crate is a test of the broker: an output ceiling that
//! bounds peak memory, a write that reverts on cancel, a child whose grandchild
//! dies with it. Against a stub those assertions would be assertions about the
//! stub, which is exactly why plan 06's Task 4 waited for `orrery-broker`.
//!
//! So this rig is the genuine path: `PolicyEngine::check` mints a
//! [`CapabilityToken`](orrery_policy::CapabilityToken), `LocalBroker` redeems
//! it, and the bytes move through `LimitedReader` and `WriteHandle`. The only
//! thing it stands in for is the wiring `orrery-harness` does in production.
//!
//! # Why the facade is per-call here
//!
//! `orrery_ext_api::CallCtx` carries the call's id and its cancel token;
//! `ExtensionTable` carries **one** `Arc<dyn BrokerFacade>` for every call it
//! serves. So a broker cannot ask "which call is this?" from inside a
//! `read`/`write`/`spawn`. This rig builds one facade per call, which is what
//! the production wiring should end up doing too — see the note in
//! `orrery-harness`'s `broker` module.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use orrery_broker::{Broker, LocalBroker, SpawnSpec};
use orrery_ext_api::{
    BrokerError, BrokerFacade, BrokerResult, CallCtx, ReadChunk, ReadRequest, SpawnOutput,
    SpawnRequest, ToolBudget as ExtBudget, WriteRequest,
};
use orrery_policy::{Decision, PendingCall, PolicyBuilder, PolicyEngine};
use orrery_proto::{AgentScope, BranchId, CallId, CancelReason, Grant, Layer, Subject};
use orrery_tools::ToolBudget;
use tokio_util::sync::CancellationToken;

/// How many bytes are handed to `WriteHandle` at a time.
///
/// Small on purpose: the atomicity test cancels *between* chunks, and a write
/// that went out in one syscall would have nothing to cancel in the middle of.
const WRITE_CHUNK: usize = 4 * 1024;

/// A broker that really touches the disk, behind a real policy engine.
pub struct TestBroker {
    engine: Arc<PolicyEngine>,
    broker: LocalBroker,
    subject: Subject,
    scope: AgentScope,
    budget: ToolBudget,
    cancel: CancellationToken,
    /// The high-water mark of bytes actually pulled from a source, so a test can
    /// assert on the **peak** rather than on the result.
    peak_pull: Arc<AtomicU64>,
}

impl TestBroker {
    /// A broker over a workspace, allowing read, write and spawn inside it.
    #[must_use]
    pub fn open(root: &Path, budget: ToolBudget, cancel: CancellationToken) -> Arc<Self> {
        let rules = PolicyBuilder::new(root)
            .layer_toml(
                "[permissions]\nallow = [\"read(./**)\", \"write(./**)\", \"spawn(*)\", \"tool(*)\"]\n",
                "test.toml",
                Layer::Project,
                true,
            )
            .expect("the test rule file parses")
            .build()
            .expect("the test rules compile");
        let engine = Arc::new(PolicyEngine::new(rules));
        let broker = LocalBroker::new(engine.ledger().clone());
        Arc::new(Self {
            engine,
            broker,
            subject: Subject::Agent,
            scope: AgentScope {
                agent: "main".into(),
                branch: BranchId::new(),
                tools: vec!["*".into()],
                grant: Grant::nothing(),
            },
            budget,
            cancel,
            peak_pull: Arc::new(AtomicU64::new(0)),
        })
    }

    /// The most bytes any one read pulled from its source.
    #[must_use]
    pub fn peak_pull(&self) -> u64 {
        self.peak_pull.load(Ordering::SeqCst)
    }

    /// A call context wired to this broker.
    #[must_use]
    pub fn ctx(self: &Arc<Self>, tool: &str) -> CallCtx {
        CallCtx::new(
            CallId::new(),
            "builtin".parse().expect("`builtin` is an ext id"),
            tool,
            ExtBudget {
                wall_clock_ms: self.budget.wall_clock_ms,
                output_bytes: self.budget.output_bytes,
                memory_bytes: self.budget.memory_bytes,
            },
            self.cancel.clone(),
            self.clone(),
        )
    }

    /// Ask the real engine, and keep the token it mints.
    fn allow(&self, call: &PendingCall) -> BrokerResult<orrery_policy::CapabilityToken> {
        match self.engine.check(call, &self.subject, &self.scope) {
            Decision::Allow { token, .. } => Ok(token),
            Decision::Deny { rule, reason } => Err(BrokerError::Denied { rule, reason }),
            other => Err(BrokerError::Denied {
                rule: other.rule(),
                reason: "this call needs consent, and none was given".to_owned(),
            }),
        }
    }
}

fn io(e: impl std::fmt::Display) -> BrokerError {
    BrokerError::Io {
        message: e.to_string(),
    }
}

#[async_trait]
impl BrokerFacade for TestBroker {
    async fn read(&self, req: ReadRequest) -> BrokerResult<ReadChunk> {
        let token = self.allow(&PendingCall::read(req.path.display().to_string()))?;
        let ceiling = req.limit.saturating_add(req.offset);
        let budget = ToolBudget::new(self.budget.wall_clock_ms, ceiling);
        let mut reader = self
            .broker
            .read(token, &req.path, &budget)
            .await
            .map_err(io)?;
        let counter = reader.counter();
        let (bytes, truncated) = reader.take_bytes().await.map_err(io)?;
        self.peak_pull.fetch_max(counter.pulled(), Ordering::SeqCst);

        let offset = usize::try_from(req.offset).unwrap_or(usize::MAX);
        let bytes = bytes.get(offset.min(bytes.len())..).unwrap_or(&[]).to_vec();
        let total = std::fs::metadata(&req.path).ok().map(|m| m.len());
        Ok(ReadChunk {
            bytes,
            eof: !truncated,
            total,
        })
    }

    async fn write(&self, req: WriteRequest) -> BrokerResult<()> {
        let token = self.allow(&PendingCall::write(req.path.display().to_string()))?;
        let mut handle = self
            .broker
            .write(token, &req.path, req.atomic)
            .await
            .map_err(io)?;
        for chunk in req.contents.chunks(WRITE_CHUNK) {
            if self.cancel.is_cancelled() {
                // Dropping the handle would do this anyway; saying it out loud
                // is what the atomicity test is about.
                handle.cancel().await;
                return Err(BrokerError::Cancelled {
                    reason: CancelReason::User,
                });
            }
            handle.write_all(chunk).await.map_err(io)?;
            tokio::task::yield_now().await;
        }
        if self.cancel.is_cancelled() {
            handle.cancel().await;
            return Err(BrokerError::Cancelled {
                reason: CancelReason::User,
            });
        }
        handle.commit().await.map_err(io)
    }

    async fn spawn(&self, req: SpawnRequest) -> BrokerResult<SpawnOutput> {
        let mut spec = SpawnSpec::new(&req.program).args(req.args.clone());
        if let Some(cwd) = &req.cwd {
            spec = spec.cwd(cwd.clone());
        }
        let token = self.allow(&PendingCall::spawn(spec.command_text()))?;
        let budget = ToolBudget {
            wall_clock_ms: req.timeout_ms.unwrap_or(self.budget.wall_clock_ms),
            output_bytes: req.output_bytes.unwrap_or(self.budget.output_bytes),
            memory_bytes: self.budget.memory_bytes,
        };
        let child = self.broker.spawn(token, spec, &budget).await.map_err(io)?;
        let out = child.wait().await.map_err(io)?;
        Ok(SpawnOutput {
            status: out.code,
            stdout: out.stdout,
            stderr: out.stderr,
            truncated: out.truncated,
        })
    }
}

/// The `contain_probe` example, built beside the test binary.
///
/// `cargo test` builds a crate's examples, so the binary is a sibling of the
/// test's own executable — the same trick `orrery-ext-session-sqlite` uses for
/// its crash test.
#[must_use]
pub fn probe_exe() -> PathBuf {
    let mut dir = std::env::current_exe().expect("a test binary knows where it is");
    dir.pop();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir.join("examples")
        .join(format!("contain_probe{}", std::env::consts::EXE_SUFFIX))
}
