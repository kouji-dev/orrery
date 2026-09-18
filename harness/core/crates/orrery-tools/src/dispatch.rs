//! The one dispatch path, and the three things it talks to.
//!
//! `dispatch` is the only public way to reach a [`ToolHost`]. The host is a
//! private field of [`Registry`] and no public method returns it, so there is
//! no path around the policy check in step 4.

use async_trait::async_trait;
use orrery_proto::{AgentScope, CallId, Outcome, RuleId, Subject, ToolRef, Verdict};

use crate::budget::ToolBudget;
use crate::error::ToolError;
use crate::registry::Registry;

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
/// # Dispatch is the only way to reach one
///
/// The registry holds its host in a private field and offers no public method
/// that hands it out, so there is no route to a tool that skips the policy
/// check:
///
/// ```compile_fail
/// # use orrery_tools::Registry;
/// let registry = Registry::new();
/// let host = registry.host;    // private field
/// let host = registry.host();  // private method
/// ```
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


impl Registry {
    /// Run one tool call, all seven steps, and report how it went.
    ///
    /// ```
    /// # use std::sync::Arc;
    /// # use orrery_proto::{AgentScope, BranchId, CallId, Grant, Layer, Subject, ToolRef};
    /// # use orrery_tools::{CallCtx, Registry, ToolBudget, ToolSpec};
    /// # async fn example() {
    /// let mut registry = Registry::new();
    /// registry.register(&"builtin".parse().unwrap(), Layer::Project, ToolSpec::new("read"));
    ///
    /// let scope = AgentScope {
    ///     agent: "main".into(),
    ///     branch: BranchId::new(),
    ///     tools: vec!["builtin.*".into()],
    ///     grant: Grant::nothing(),
    /// };
    /// let ctx = CallCtx::new(
    ///     CallId::new(),
    ///     Subject::Agent,
    ///     scope,
    ///     ToolBudget::new(30_000, 1 << 20),
    /// );
    /// let r#ref: ToolRef = "builtin.read".parse().unwrap();
    /// let outcome = registry.dispatch(&r#ref, serde_json::json!({}), ctx).await;
    /// assert!(outcome.is_ok());
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// [`ToolError`] when the harness itself could not carry the call: an
    /// unknown reference, an unusable schema, an unreachable host. A refusal is
    /// **not** an error — it is `Ok(Outcome::Denied)`.
    pub async fn dispatch(
        &self,
        r#ref: &ToolRef,
        input: serde_json::Value,
        ctx: CallCtx,
    ) -> Result<Outcome, ToolError> {
        // 1 · `tool.resolve` interceptors.
        let mut r#ref = r#ref.clone();
        for i in &self.interceptors {
            match i.on_resolve(&r#ref) {
                Verdict::Continue => {}
                Verdict::Rewrite(next) => r#ref = next,
                Verdict::Deny { reason } => return Ok(denied(reason)),
                Verdict::Handled { result } => return Ok(result),
                // `Verdict` is `#[non_exhaustive]`: a variant this crate has
                // not been taught about is not a licence to skip a step.
                _ => {}
            }
        }

        let entry = self
            .entries
            .get(&r#ref)
            .ok_or_else(|| ToolError::NoSuchTool {
                name: r#ref.to_string(),
            })?;

        // A tool the scope was never offered is refused, not called. This is
        // what makes `visible` a real subset rather than a prompt-level hint.
        if !self.is_callable(&r#ref, &ctx.scope) {
            return Ok(denied(format!(
                "`{ref}` is not in the tool set of agent `{agent}`",
                r#ref = r#ref,
                agent = ctx.scope.agent
            )));
        }

        // 2 · The input is validated at the boundary. A malformed call is a
        //     `Failed` outcome; it never panics and never reaches the host.
        if let Err(message) = validate(&entry.spec.input_schema, &input, &r#ref)? {
            return Ok(Outcome::Failed {
                code: "invalid-input".to_owned(),
                message,
            });
        }

        // 3 · `tool.before` interceptors. A `Deny` here narrows.
        let mut input = input;
        for i in &self.interceptors {
            match i.before(&r#ref, &input) {
                Verdict::Continue => {}
                Verdict::Rewrite(next) => input = next,
                Verdict::Deny { reason } => return Ok(denied(reason)),
                Verdict::Handled { result } => return Ok(result),
                // `Verdict` is `#[non_exhaustive]`: a variant this crate has
                // not been taught about is not a licence to skip a step.
                _ => {}
            }
        }

        // 4 · The policy check. There is no path around this.
        if let PolicyDecision::Deny { rule, reason } = self.policy.check(&r#ref, &input, &ctx) {
            return Ok(Outcome::Denied { rule, reason });
        }

        // 5 · The call itself, under the ceiling the manifest declared.
        let ctx = match entry.spec.ceiling {
            Some(ceiling) => ctx.narrowed_to(ceiling),
            None => ctx,
        };
        let mut outcome = self.host().call(&r#ref, input, &ctx).await?;

        // 6 · `tool.after` interceptors.
        for i in &self.interceptors {
            match i.after(&r#ref, &outcome) {
                Verdict::Continue => {}
                Verdict::Rewrite(next) => outcome = next,
                Verdict::Deny { reason } => return Ok(denied(reason)),
                Verdict::Handled { result } => return Ok(result),
                // `Verdict` is `#[non_exhaustive]`: a variant this crate has
                // not been taught about is not a licence to skip a step.
                _ => {}
            }
        }

        // 7 · Audit, and return.
        // TODO(plan-07): this event belongs in `orrery-audit`'s stream. Until
        // that crate exists as a dependency, tracing carries it.
        tracing::info!(
            target: "orrery.tools.dispatch",
            tool = %r#ref,
            call = %ctx.call,
            subject = %ctx.subject,
            ok = outcome.is_ok(),
            "tool call settled"
        );
        Ok(outcome)
    }
}

/// The rule id a refusal carries when no configured rule is responsible for it
/// — the registry's own "not in your tool set" and an interceptor's `Deny`.
const NO_RULE: &str = "00000000-0000-0000-0000-000000000000";

/// A refusal that did not come from a numbered rule.
fn denied(reason: impl Into<String>) -> Outcome {
    Outcome::Denied {
        rule: NO_RULE.parse().expect("the nil uuid is a uuid"),
        reason: reason.into(),
    }
}

/// `Ok(Ok(()))` when the input fits, `Ok(Err(message))` when it does not,
/// `Err` when the schema itself is unusable.
#[cfg(feature = "schema-validation")]
fn validate(
    schema: &serde_json::Value,
    input: &serde_json::Value,
    r#ref: &ToolRef,
) -> Result<Result<(), String>, ToolError> {
    let validator = jsonschema::validator_for(schema).map_err(|e| ToolError::InvalidSchema {
        name: r#ref.to_string(),
        message: e.to_string(),
    })?;
    match validator.validate(input) {
        Ok(()) => Ok(Ok(())),
        Err(e) => Ok(Err(e.to_string())),
    }
}

#[cfg(not(feature = "schema-validation"))]
fn validate(
    _schema: &serde_json::Value,
    _input: &serde_json::Value,
    _ref: &ToolRef,
) -> Result<Result<(), String>, ToolError> {
    Ok(Ok(()))
}
