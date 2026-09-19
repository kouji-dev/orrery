//! The facade: builds a Kernel from resolved config, owns the tokio runtime, links the first-party set behind features.
//!
//! # What this crate is for
//!
//! One `build` call turns a config into a running kernel: the policy engine, the
//! broker, the extension table, the tool registry, the session store and the
//! provider, wired in one order that is written down once. Everything else in
//! the system takes its pieces as `Arc<dyn …>` and does not know how they were
//! chosen.
//!
//! # The bridge for sync embedders
//!
//! [`Harness::block_on`] exists for the ADE: a Tauri command is sync, the kernel
//! is async, and the alternative to a bridge here is a runtime handle passed
//! around by hand in every embedder. [`Harness::handle`] is the async half, for
//! an embedder that has its own runtime.
//!
//! # The only core crate that names an `extensions/` crate
//!
//! Cargo features select the first-party set — `builtin-tools`, `sqlite`,
//! `fixture-provider`, `anthropic` — and every `#[cfg]` for them lives in
//! [`features`]. `cargo xtask deps-check` allow-lists this crate by name;
//! anything else under `core/` that reached into `extensions/` would be a
//! layering violation.
//!
//! Implementation plan: `harness/docs/plans/05-kernel-loop.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod broker;
pub mod build;
pub mod config;
pub mod features;
#[cfg(feature = "fixture-provider")]
pub mod fixture;
pub mod mcp;
pub mod memory;
pub mod plan;
pub mod skills;
pub mod steps;
pub mod surfaces;

use std::sync::Arc;

use orrery_ext_api::Ledger;
use orrery_kernel::{Kernel, TurnInput, TurnOutcome};
use orrery_policy::PolicyEngine;
use orrery_proto::{AgentScope, BranchId, SessionId, UserInput};
use orrery_session::SessionStore;
use tokio_util::sync::CancellationToken;

pub use broker::{LedgerRevoker, PolicyBroker};
pub use build::{
    BuildError, DEFAULT_RULES, ExtensionSource, ProviderChoice, ResolvedConfig, StoreChoice,
    default_tool_budget, extension_sources, provider_for,
};
pub use config::{kernel_config, price_table};
pub use memory::KernelMemory;
pub use plan::{Skip, skip_for};
pub use steps::KernelSteps;
pub use surfaces::{KernelSurfaces, NoPatches, SurfacePatches};

/// A built harness: a runtime, a kernel, and the session it opened.
pub struct Harness {
    runtime: tokio::runtime::Runtime,
    kernel: Arc<Kernel>,
    registry: Arc<orrery_tools::Registry>,
    router: orrery_router::Router,
    surfaces: surfaces::KernelSurfaces,
    store: Arc<dyn SessionStore>,
    engine: Arc<PolicyEngine>,
    ledger: Ledger,
    session: SessionId,
    branch: BranchId,
    scope: AgentScope,
}

impl std::fmt::Debug for Harness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Harness")
            .field("session", &self.session)
            .field("kernel", &self.kernel)
            .finish_non_exhaustive()
    }
}

impl Harness {
    /// Assemble everything, and open a session.
    ///
    /// # Errors
    ///
    /// [`BuildError`] when the runtime will not start, the store will not open,
    /// the rules will not compile, a provider will not load, or a first-party
    /// extension contributed nothing at all.
    pub fn build(config: ResolvedConfig) -> Result<Self, BuildError> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(BuildError::Runtime)?;
        let assembled = runtime.block_on(build::assemble(&config))?;
        Ok(Self {
            runtime,
            kernel: assembled.kernel,
            registry: assembled.registry,
            router: assembled.router,
            surfaces: assembled.surfaces,
            store: assembled.store,
            engine: assembled.engine,
            ledger: assembled.ledger,
            session: assembled.session,
            branch: assembled.branch,
            scope: assembled.scope,
        })
    }

    /// The runtime, for an embedder that has none.
    #[must_use]
    pub fn handle(&self) -> tokio::runtime::Handle {
        self.runtime.handle().clone()
    }

    /// The bridge for sync embedders — the ADE's Tauri commands.
    pub fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.runtime.block_on(f)
    }

    /// The kernel.
    #[must_use]
    pub fn kernel(&self) -> Arc<Kernel> {
        self.kernel.clone()
    }

    /// The turn tree.
    #[must_use]
    pub fn store(&self) -> &Arc<dyn SessionStore> {
        &self.store
    }

    /// The tools a turn can reach, for a caller that dispatches one itself —
    /// a workflow's `tool` step does, and it makes no model call to do it.
    #[must_use]
    pub fn registry(&self) -> Arc<orrery_tools::Registry> {
        self.registry.clone()
    }

    /// The kernel's surface store, for the composition root that has to tell
    /// it when a turn starts and when it seals.
    #[must_use]
    pub fn surfaces(&self) -> &surfaces::KernelSurfaces {
        &self.surfaces
    }

    /// The router the declared `[[route]]` rules built.
    #[must_use]
    pub fn router(&self) -> &orrery_router::Router {
        &self.router
    }

    /// Everything a workflow's steps reach the system through.
    ///
    /// Separate from [`run_workflow`](Self::run_workflow) so a caller that
    /// wants to observe the steps can build its own
    /// [`Runner`](orrery_orchestrator::Runner) over it.
    #[must_use]
    pub fn steps(&self, cancel: CancellationToken) -> steps::KernelSteps {
        steps::KernelSteps::new(
            self.kernel.clone(),
            self.store.clone(),
            self.registry.clone(),
            self.session,
            self.branch,
            self.scope.clone(),
            self.router.clone(),
        )
        .cancelled_by(cancel)
    }

    /// Run a typechecked workflow to its end.
    ///
    /// This is the product's reach into plan 11: `orrery run -p` submits
    /// exactly one turn and has nowhere to put a workflow, so without this the
    /// loop machine, its caps and the router were library tests and nothing
    /// else. A loop that never satisfies its predicate stops at
    /// `max_iterations` on a counter the orchestrator owns — not on a prompt's
    /// good behaviour — and the cap is in the answer.
    pub async fn run_workflow(
        &self,
        workflow: &orrery_orchestrator::Checked,
        budget: Option<orrery_proto::Budget>,
        cancel: CancellationToken,
    ) -> orrery_orchestrator::WorkflowRun {
        let steps = self.steps(cancel.clone());
        orrery_orchestrator::Runner::new(&steps)
            .cancelled_by(cancel)
            .run(workflow, budget)
            .await
    }

    /// The policy engine, for `permissions explain` and for a reload.
    #[must_use]
    pub fn engine(&self) -> &Arc<PolicyEngine> {
        &self.engine
    }

    /// What loaded, what degraded, what failed, what was skipped.
    #[must_use]
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// The session this harness opened.
    #[must_use]
    pub fn session(&self) -> SessionId {
        self.session
    }

    /// Its root branch.
    #[must_use]
    pub fn branch(&self) -> BranchId {
        self.branch
    }

    /// The main agent's scope.
    #[must_use]
    pub fn scope(&self) -> &AgentScope {
        &self.scope
    }

    /// Submit one turn on the root branch and wait for it.
    ///
    /// The shortest path from a string to a finished turn, which is what
    /// `orrery run -p "…"` (plan 17) is.
    ///
    /// # Errors
    ///
    /// [`orrery_kernel::KernelError`] when the harness itself broke. A ceiling,
    /// a refusal, a cancellation and a login prompt are all
    /// [`TurnOutcome`] values.
    pub async fn submit(
        &self,
        text: impl Into<String>,
        cancel: CancellationToken,
    ) -> Result<TurnOutcome, orrery_kernel::KernelError> {
        let lease = self.store.lease(self.branch).await?;
        self.kernel
            .run_turn(
                lease,
                TurnInput::new(self.session, UserInput::text(text), self.scope.clone()),
                cancel,
            )
            .await
    }
}
