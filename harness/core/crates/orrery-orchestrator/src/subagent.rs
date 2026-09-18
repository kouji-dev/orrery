//! Sub-agents run on branches, and **the parent merges at the join**.
//!
//! # The one deadlock in the design, and the shape that avoids it
//!
//! Cross-reference plan 02 task 8. A sub-agent runs on a child branch while its
//! parent still holds the parent branch's lease — the parent is mid-turn; that
//! is why it spawned a child at all. So:
//!
//! - [`branch`](orrery_session::SessionStore::branch) takes **no lease**.
//! - [`close_branch`](orrery_session::SessionStore::close_branch) consumes the
//!   **child's** lease and writes only the child's rows.
//! - The parent, still holding its own lease, appends the
//!   [`BranchResult`](orrery_session::TurnKind::BranchResult) itself.
//!
//! A child never appends to its parent. [`spawn`] is written against exactly
//! that order, and `subagent::parent_merges_at_the_join` asserts it under a
//! timeout, because "no deadlock" is not something an assertion on the happy
//! path can show.
//!
//! # A sub-agent's work is ordinary turns
//!
//! The child's passes are appended to the child branch as ordinary
//! [`TurnKind`](orrery_session::TurnKind) rows: inspectable, replayable, and
//! **not** collapsed into one tool result. That is the whole reason a sub-agent
//! is a branch rather than a function call.

use async_trait::async_trait;
use orrery_proto::{AgentScope, BranchId, TurnId, Usage, UserInput};
use orrery_router::{RoleAgentDef, bind_scope};
use orrery_session::{BranchLease, BranchOutcome, NewTurn, SessionStore, TurnKind};

use crate::step::AgentDefinition;

/// What running a sub-agent's turns looks like from here.
///
/// The orchestrator does not know how to run a turn — that is the kernel's job
/// — so it asks through this. The implementation appends the child's turns to
/// the child branch under the lease it is handed.
#[async_trait]
pub trait TurnRunner: Send + Sync {
    /// Run the child's work on its own branch, under its own scope.
    async fn run(
        &self,
        lease: &BranchLease,
        scope: &AgentScope,
        input: UserInput,
    ) -> Result<TurnReport, SubAgentError>;
}

/// What a child's run came to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurnReport {
    /// Its last word, which becomes the branch's summary.
    pub text: String,
    /// What it cost.
    pub usage: Usage,
    /// How many turns it took.
    pub turns: u32,
}

/// A sub-agent that did not run, as a **typed** value the parent can handle.
///
/// [`CannotPrompt`](SubAgentError::CannotPrompt) is the interesting one: a
/// sub-agent on a branch has nobody attached to it, so a consent prompt has
/// nowhere to go. Auto-denying would turn that into a stall the parent cannot
/// tell from a slow child, so it comes back as a value instead and the parent
/// decides — ask on its own branch, run the step itself, or give up.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SubAgentError {
    /// The child needed to ask a person, and cannot.
    #[error("`{agent}` needs to ask about {what}, and a sub-agent branch has nobody attached")]
    CannotPrompt {
        /// Which agent.
        agent: String,
        /// What it wanted to ask about.
        what: String,
    },
    /// The child ran and failed.
    #[error("`{agent}` failed: {code}: {message}")]
    Failed {
        /// Which agent.
        agent: String,
        /// A stable code.
        code: String,
        /// What went wrong.
        message: String,
    },
    /// The session store would not answer.
    #[error("the session store: {message}")]
    Store {
        /// What it said.
        message: String,
    },
}

impl From<orrery_session::SessionError> for SubAgentError {
    fn from(e: orrery_session::SessionError) -> Self {
        SubAgentError::Store {
            message: e.to_string(),
        }
    }
}

/// What a completed sub-agent leaves behind.
#[derive(Clone, Debug, PartialEq)]
pub struct SubAgentResult {
    /// The branch it ran on. Its turns are still there.
    pub branch: BranchId,
    /// The scope it actually ran under — the parent's, narrowed.
    pub scope: AgentScope,
    /// How the branch ended.
    pub outcome: BranchOutcome,
    /// The join row the **parent** wrote.
    pub join: TurnId,
    /// What the child cost.
    pub usage: Usage,
}

/// Run a sub-agent on a child branch and merge it at the join.
///
/// The order is the whole function, and it is the order plan 02 requires:
/// narrow the scope, fork (no lease), lease the **child**, run, close the child,
/// then — parent lease in hand the entire time — append the `BranchResult`.
///
/// # Errors
///
/// [`SubAgentError`] when the store refuses, or when the child could not run.
/// Either way the child branch is closed and the parent's join row is written
/// first, so a failure leaves the tree in the same shape a success does.
pub async fn spawn(
    store: &dyn SessionStore,
    parent_lease: &BranchLease,
    at: TurnId,
    def: &AgentDefinition,
    parent_scope: &AgentScope,
    input: UserInput,
    runner: &dyn TurnRunner,
) -> Result<SubAgentResult, SubAgentError> {
    // 1 · the grant is intersected with the parent's. A sub-agent declaring a
    //     wider grant gets the intersection; it cannot do what the parent could
    //     not.
    let mut scope = bind_scope(
        parent_scope,
        &RoleAgentDef::named(&def.name)
            .asking(def.grant.clone())
            .seeing(def.tools.clone()),
    );

    // 2 · fork. Takes no lease, because forking is a read of the parent.
    let branch = store.branch(at, &def.name).await?;
    scope.branch = branch;

    // 3 · the child's own lease. The parent is still holding its own.
    let child_lease = store.lease(branch).await?;

    // 4 · the child's turns, appended to the child branch as ordinary rows.
    let ran = runner.run(&child_lease, &scope, input).await;

    let (outcome, usage, error) = match ran {
        Ok(report) => (
            BranchOutcome::Completed {
                summary: report.text,
            },
            report.usage,
            None,
        ),
        Err(e) => {
            let (code, message) = match &e {
                SubAgentError::CannotPrompt { .. } => ("cannot-prompt".to_owned(), e.to_string()),
                SubAgentError::Failed { code, message, .. } => (code.clone(), message.clone()),
                SubAgentError::Store { message } => ("store".to_owned(), message.clone()),
            };
            (
                BranchOutcome::Failed { code, message },
                Usage::default(),
                Some(e),
            )
        }
    };

    // 5 · close the child. Consumes the child's lease, writes only the child's
    //     rows, and never touches the parent.
    store.close_branch(child_lease, outcome.clone()).await?;

    // 6 · the parent performs the join, under its own lease.
    let join = store
        .append(
            parent_lease,
            NewTurn::new(TurnKind::BranchResult {
                child: branch,
                outcome: outcome.clone(),
            }),
        )
        .await?;

    match error {
        // The tree is consistent either way: the child is closed and the join
        // row is written before the typed error goes back to the parent.
        Some(e) => Err(e),
        None => Ok(SubAgentResult {
            branch,
            scope,
            outcome,
            join,
            usage,
        }),
    }
}
