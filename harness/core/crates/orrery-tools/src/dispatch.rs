//! The one dispatch path, and the three things it talks to.
//!
//! `dispatch` is the only public way to reach a [`ToolHost`]. The host is a
//! private field of [`Registry`] and no public method returns it, so there is
//! no path around the policy check in step 4.

use async_trait::async_trait;
use orrery_proto::{AgentScope, CallId, Outcome, RuleId, Subject, ToolRef, Verdict};

use crate::budget::ToolBudget;
use crate::error::ToolError;

/// Everything one call needs that is not its input.
///
/// There is **no constructor without a [`ToolBudget`]**. Objective 5 is that a
/// tool call always has a ceiling, and the way to make that true is to make the
/// budget-less call impossible to write rather than to remember to pass one:
///
/// ```compile_fail
/// # use orrery_tools::CallCtx;
/// # use orrery_proto::{AgentScope, BranchId, CallId, Grant, Subject};
/// let scope = AgentScope {
///     agent: "a".into(),
///     branch: BranchId::new(),
///     tools: vec!["*".into()],
///     grant: Grant::nothing(),
/// };
/// // No budget: does not compile.
/// let ctx = CallCtx::new(CallId::new(), Subject::Agent, scope);
/// ```
#[derive(Clone, Debug)]
pub struct CallCtx {
    /// Which call.
    pub call: CallId,
    /// Who is calling.
    pub subject: Subject,
    /// What they may see.
    pub scope: AgentScope,
    budget: ToolBudget,
}

impl CallCtx {
    /// A context for one call. The budget is not optional.
    #[must_use]
    pub fn new(call: CallId, subject: Subject, scope: AgentScope, budget: ToolBudget) -> Self {
        Self {
            call,
            subject,
            scope,
            budget,
        }
    }

    /// The ceiling this call runs under.
    #[must_use]
    pub fn budget(&self) -> ToolBudget {
        self.budget
    }

    /// Narrow the ceiling. Widening is not offered.
    #[must_use]
    pub fn narrowed_to(mut self, budget: ToolBudget) -> Self {
        self.budget = self.budget.narrow(budget);
        self
    }
}

/// Whatever actually runs a tool.
///
/// TODO(plan-06): the extension host implements this. Until then the registry
/// ships [`UnavailableHost`], so every path below dispatch already exists and
/// plan 06 drops in without touching the dispatch order.
#[async_trait]
pub trait ToolHost: Send + Sync {
    /// Run the tool and report how it went.
    async fn call(
        &self,
        r#ref: &ToolRef,
        input: serde_json::Value,
        ctx: &CallCtx,
    ) -> Result<Outcome, ToolError>;
}

/// The host of a registry that has none: every call comes back `Unloaded`.
#[derive(Copy, Clone, Debug, Default)]
pub struct UnavailableHost;

#[async_trait]
impl ToolHost for UnavailableHost {
    async fn call(
        &self,
        r#ref: &ToolRef,
        _input: serde_json::Value,
        _ctx: &CallCtx,
    ) -> Result<Outcome, ToolError> {
        Ok(Outcome::Unloaded {
            ext: r#ref.ext.clone(),
        })
    }
}

/// What the policy engine said about one call.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Carry on.
    Allow,
    /// Do not. Becomes `Ok(Outcome::Denied)`, never an error.
    Deny {
        /// Which rule refused.
        rule: RuleId,
        /// Why, in words a person can act on.
        reason: String,
    },
}

/// The check step 4 runs, which nothing can skip.
///
/// TODO(plan-07): `orrery-policy` implements this. Until then the registry
/// ships [`AllowAll`], so the dispatch path is already shaped around the check
/// and plan 07 is a substitution rather than a rewrite.
pub trait PolicyCheck: Send + Sync {
    /// May this subject make this call, with this input?
    fn check(&self, r#ref: &ToolRef, input: &serde_json::Value, ctx: &CallCtx) -> PolicyDecision;

    /// Would this subject be refused this tool whatever the input?
    ///
    /// Only the categorical answer, and only for `visible`: a tool the subject
    /// could never call should not be offered to the model at all. A per-call
    /// decision still goes through [`PolicyCheck::check`].
    fn categorically_denies(&self, _subject: &Subject, _ref: &ToolRef) -> bool {
        false
    }
}

/// The stand-in policy: everything is allowed.
#[derive(Copy, Clone, Debug, Default)]
pub struct AllowAll;

impl PolicyCheck for AllowAll {
    fn check(&self, _ref: &ToolRef, _input: &serde_json::Value, _ctx: &CallCtx) -> PolicyDecision {
        PolicyDecision::Allow
    }
}

/// The `tool.resolve`, `tool.before` and `tool.after` phases.
///
/// TODO(plan-05): the kernel owns the interceptor chain and its `Phase` trait.
/// This is the tool-side slice of it, so the dispatch order already has the
/// three hook points in the right places.
pub trait ToolInterceptor: Send + Sync {
    /// Step 1: rewrite or refuse a resolved reference.
    fn on_resolve(&self, _ref: &ToolRef) -> Verdict<ToolRef> {
        Verdict::Continue
    }

    /// Step 3: rewrite or refuse an input. A `Deny` here narrows.
    fn before(&self, _ref: &ToolRef, _input: &serde_json::Value) -> Verdict<serde_json::Value> {
        Verdict::Continue
    }

    /// Step 6: rewrite an outcome on the way back.
    fn after(&self, _ref: &ToolRef, _outcome: &Outcome) -> Verdict<Outcome> {
        Verdict::Continue
    }
}

