//! Steps, branch runs on leases, the workflow machine, and load-time dataflow typechecking.
//!
//! # The four things this crate makes true
//!
//! - **The whole dataflow is typechecked at load** (translation #5). Step *N*
//!   may only `ref` steps `< N`, and each ref's path must typecheck against the
//!   target's declared `returns`. A workflow must not fail mid-run on a type
//!   error after paying for three model calls — [`typecheck`].
//! - **Every loop declares a predicate and a hard cap, and the cap is
//!   enforced here.** A verify loop whose gate never passes stops at
//!   `max_iterations` with the cap recorded, on a counter this machine owns and
//!   not on a prompt's good behaviour — [`workflow`].
//! - **The parent merges at the join.** A child branch never appends to its
//!   parent; the parent writes the `BranchResult` under its own lease, which is
//!   what keeps a sub-agent call from deadlocking — [`subagent`].
//! - **Deterministic steps cost nothing.** A [`Step::Tool`] makes no model call
//!   at all, which is where the cost comes out.
//!
//! # It does not depend on the kernel
//!
//! A step's passes are run through [`TurnRunner`] and [`StepExecutor`], which
//! the facade implements over `orrery-kernel`. Together with
//! `orrery-router`'s sync `decide`, that is what keeps kernel, orchestrator and
//! router acyclic.
//!
//! Implementation plan: `harness/docs/plans/11-router-roles-orchestrator.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod budget;
pub mod expr;
pub mod join;
pub mod step;
pub mod subagent;
pub mod typecheck;
pub mod workflow;

pub use budget::{Exceeded, WorkflowBudget};
pub use expr::{Env, ExprError, eval, holds};
pub use join::{Join, Joined};
pub use step::{AgentDefinition, Catalogue, NamedStep, OnFail, Step, ToolDefinition, TypeShape};
pub use subagent::{SubAgentError, SubAgentResult, TurnReport, TurnRunner, spawn};
pub use typecheck::{BadPath, LoadError, Workflow, check};
pub use workflow::{
    CapReached, Outcome, Runner, StepCtx, StepExecutor, StepFailure, StepObserver, StepOutput,
    WorkflowRun,
};
