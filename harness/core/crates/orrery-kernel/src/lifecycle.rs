//! The other half of §4.1: handlers that may do I/O and may not change anything.
//!
//! An interceptor is sync, returns a verdict and cannot touch the world. A
//! lifecycle handler is the mirror image: it is async, it holds a broker, and it
//! returns **nothing**. That asymmetry is the design. A handler at `turn.end`
//! can write to memory; it cannot alter the turn it fired on, because there is
//! no channel for it to do so — `run` returns `Result<(), LifecycleError>` and
//! the `Ok` half is a unit.
//!
//! `session.start` appears in both lists on purpose: an interceptor there
//! returns a verdict on the resolved manifest, a handler there does the I/O of
//! opening a store.
//!
//! # A failing handler never fails the turn
//!
//! It is logged, it is audited, and it is **disabled for the rest of the
//! session**. A handler that throws once usually throws every time, and a turn
//! that dies because somebody's memory backend is down is a turn lost to
//! something that was never load-bearing.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use orrery_audit::{Audit, AuditEvent, ContentRef};
use orrery_proto::{AgentScope, BranchId, SessionId, TurnId, Usage};

/// Where a handler fires.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum LifecyclePoint {
    /// A session opened.
    SessionStart,
    /// A session closed.
    SessionEnd,
    /// A turn settled, whatever it settled as.
    TurnEnd,
    /// A branch closed.
    BranchClose,
    /// A workflow finished.
    WorkflowEnd,
}

impl LifecyclePoint {
    /// The dotted name, as a manifest writes it and the ledger prints it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            LifecyclePoint::SessionStart => "session.start",
            LifecyclePoint::SessionEnd => "session.end",
            LifecyclePoint::TurnEnd => "turn.end",
            LifecyclePoint::BranchClose => "branch.close",
            LifecyclePoint::WorkflowEnd => "workflow.end",
        }
    }
}

impl std::fmt::Display for LifecyclePoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// What a handler is told about the thing that just happened.
#[derive(Clone, Debug)]
pub struct LifecycleCtx {
    /// Which session.
    pub session: SessionId,
    /// Which branch.
    pub branch: BranchId,
    /// Which turn, when one is in play.
    pub turn: Option<TurnId>,
    /// Who was acting.
    pub agent: AgentScope,
    /// What it cost.
    pub usage: Usage,
}

/// A handler that could not do its job.
#[non_exhaustive]
#[derive(Debug, Clone, thiserror::Error)]
pub enum LifecycleError {
    /// It tried and something went wrong.
    #[error("{message}")]
    Failed {
        /// What went wrong, in words a person can act on.
        message: String,
    },
}

impl LifecycleError {
    /// A failure from anything that renders.
    #[must_use]
    pub fn failed(message: impl std::fmt::Display) -> Self {
        Self::Failed {
            message: message.to_string(),
        }
    }
}

/// Something that happens *because* of a turn, and cannot change it.
#[async_trait]
pub trait LifecycleHandler: Send + Sync {
    /// Where it fires.
    fn at(&self) -> LifecyclePoint;

    /// What to call it in the ledger and in a failure message.
    fn name(&self) -> &str {
        "lifecycle handler"
    }

    /// Do the work.
    ///
    /// **Returns nothing.** It may do I/O through the broker it was built with;
    /// it cannot alter the turn it fired on.
    ///
    /// # Errors
    ///
    /// [`LifecycleError`] when it could not. The turn is unaffected and the
    /// handler is disabled for the rest of the session.
    async fn run(&self, ctx: &LifecycleCtx) -> Result<(), LifecycleError>;
}

struct Entry {
    handler: Arc<dyn LifecycleHandler>,
    disabled: AtomicBool,
}

/// Every lifecycle handler the session has.
pub struct LifecycleSet {
    entries: Vec<Entry>,
    audit: Audit,
}

impl std::fmt::Debug for LifecycleSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleSet")
            .field("handlers", &self.entries.len())
            .field(
                "disabled",
                &self
                    .entries
                    .iter()
                    .filter(|e| e.disabled.load(Ordering::Relaxed))
                    .count(),
            )
            .finish()
    }
}

impl Default for LifecycleSet {
    fn default() -> Self {
        Self::new()
    }
}

impl LifecycleSet {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            audit: orrery_audit::null(),
        }
    }

    /// Record every failure in an audit stream.
    #[must_use]
    pub fn with_audit(mut self, audit: Audit) -> Self {
        self.audit = audit;
        self
    }

    /// Add a handler.
    pub fn register(&mut self, handler: Arc<dyn LifecycleHandler>) {
        self.entries.push(Entry {
            handler,
            disabled: AtomicBool::new(false),
        });
    }

    /// How many handlers fire at a point, counting the ones that have been
    /// disabled.
    #[must_use]
    pub fn len_at(&self, point: LifecyclePoint) -> usize {
        self.entries
            .iter()
            .filter(|e| e.handler.at() == point)
            .count()
    }

    /// What this set contributes, as the load ledger reports it.
    #[must_use]
    pub fn contributions(&self) -> Vec<orrery_proto::Contribution> {
        self.entries
            .iter()
            .map(|e| orrery_proto::Contribution {
                kind: orrery_proto::ContributionKind::Lifecycle,
                name: e.handler.at().name().to_owned(),
            })
            .collect()
    }

    /// Fire everything registered at a point.
    ///
    /// Never fails: this is called *from* the turn loop, and a handler is not
    /// allowed to end a turn.
    pub async fn fire(&self, point: LifecyclePoint, ctx: &LifecycleCtx) {
        for entry in &self.entries {
            if entry.handler.at() != point || entry.disabled.load(Ordering::Relaxed) {
                continue;
            }
            if let Err(e) = entry.handler.run(ctx).await {
                entry.disabled.store(true, Ordering::Relaxed);
                let message = format!(
                    "`{name}` failed at `{point}` and is disabled for this session: {e}",
                    name = entry.handler.name()
                );
                tracing::warn!(target: "orrery.kernel.lifecycle", %point, message);
                self.audit.append(AuditEvent::Content {
                    action: "lifecycle.failed".to_owned(),
                    content: ContentRef::new(
                        entry.handler.name().to_owned(),
                        point.name().to_owned(),
                        &message,
                    ),
                });
            }
        }
    }
}
