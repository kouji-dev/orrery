//! The one dispatch path, and the three things it talks to.
//!
//! `dispatch` is the only public way to reach a [`ToolHost`]. The host is a
//! private field of [`Registry`] and no public method returns it, so there is
//! no path around the policy check in step 4.

use async_trait::async_trait;
use orrery_audit::{AuditEvent, CallOutcome, Digest};
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
/// Plan 07 landed: `orrery_broker::EngineGate` implements this over
/// `PolicyEngine`, and `orrery_harness::build::assemble` wires it into every
/// registry the product builds. The substitution this trait was shaped for has
/// happened.
///
/// [`AllowAll`] is still here, and is **not** the product's policy: it is what a
/// `Registry::new` with no `with_policy` gets, which is a unit test of the
/// dispatch path and nothing else. A registry the harness assembles always
/// carries the gate.
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
        // Hashed **before** anything can rewrite it, so the digest is of what
        // the caller actually sent. The input itself never reaches the stream:
        // a tool call's arguments are the most likely place a secret ends up,
        // and an audit stream is the last place one should be readable.
        let input_digest = Digest::of_bytes(input.to_string().as_bytes());
        let call = ctx.call;
        // The ref as called, until an interceptor says otherwise. Auditing the
        // requested name when a `tool.resolve` interceptor rewrote it would
        // record a call that did not happen.
        let mut resolved = r#ref.clone();

        let out = self
            .dispatch_inner(r#ref, input, ctx, &mut resolved)
            .await;

        // Every settled call, including a refusal. A stream that recorded only
        // what succeeded answers "what did this agent do" and not "what did it
        // try", and the second is the question somebody asks after an incident.
        // A `ToolError` is deliberately not audited here: the call never
        // settled, and the harness failing is not the agent doing something.
        if let Ok(outcome) = &out {
            self.audit.append(AuditEvent::ToolCall {
                call,
                tool: resolved.to_string(),
                input: input_digest,
                outcome: call_outcome(outcome),
            });
        }
        out
    }

    /// The steps themselves. Split out so that every early return is audited by
    /// [`Registry::dispatch`] rather than by seven copies of the same block.
    async fn dispatch_inner(
        &self,
        r#ref: &ToolRef,
        input: serde_json::Value,
        ctx: CallCtx,
        resolved: &mut ToolRef,
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

        resolved.clone_from(&r#ref);

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

        // 7 · Return. The audit event is `dispatch`'s, so that the six earlier
        //     exits above are recorded too.
        Ok(outcome)
    }
}

/// An [`Outcome`] as the audit stream classifies it.
///
/// Four words rather than a boolean, because "it was refused", "it was
/// cancelled" and "it broke" are three different things to be looking for in a
/// stream, and `ok = false` collapses them into one.
fn call_outcome(outcome: &Outcome) -> CallOutcome {
    match outcome {
        // A truncated result is a result: the tool ran and produced something,
        // and the ceiling that cut it short is already its own event.
        Outcome::Ok { .. } | Outcome::Truncated { .. } => CallOutcome::Ok,
        Outcome::Denied { .. } => CallOutcome::Denied,
        Outcome::Cancelled { .. } => CallOutcome::Cancelled,
        // `Outcome` is `#[non_exhaustive]`. An outcome this crate has not been
        // taught about is recorded as a failure rather than as a success:
        // over-reporting a failure is recoverable, and the opposite is not.
        _ => CallOutcome::Failed,
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
