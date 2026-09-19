//! The join between the registry's dispatch and the policy engine.
//!
//! Plan 04 shipped `AllowAll` as the stub for step 4 of dispatch, with a note
//! that plan 07 substitutes the real thing. This is the substitution: it lives
//! here rather than in either crate because it is the only place that knows
//! both, and because a tool call reaching the outside world goes through this
//! crate anyway.
//!
//! It also does the load-time half — [`grant_for`] — which is what makes an
//! extension denied `spawn` **degrade** instead of failing.

use std::sync::Arc;

use orrery_policy::{Decision, PendingCall, PolicyEngine};
use orrery_proto::{AgentScope, Aspect, Capability, Consent, ExtId, Grant, Subject, ToolRef};
use orrery_tools::{CallCtx, PolicyCheck, PolicyDecision};

/// The real step 4: the policy engine, behind the registry's trait.
#[derive(Debug)]
pub struct EngineGate {
    engine: Arc<PolicyEngine>,
}

impl EngineGate {
    /// Gate every dispatch on this engine.
    #[must_use]
    pub fn new(engine: Arc<PolicyEngine>) -> Self {
        Self { engine }
    }

    /// The engine behind it.
    #[must_use]
    pub fn engine(&self) -> &Arc<PolicyEngine> {
        &self.engine
    }
}

impl PolicyCheck for EngineGate {
    fn check(&self, r#ref: &ToolRef, input: &serde_json::Value, ctx: &CallCtx) -> PolicyDecision {
        let mut call = PendingCall::tool(r#ref.to_string()).in_call(ctx.call);
        if let Some(map) = input.as_object() {
            for (k, v) in map {
                if let Some(s) = v.as_str() {
                    call = call.with_param(k, s);
                }
            }
        }
        match self.engine.check(&call, &ctx.subject, &ctx.scope) {
            // An `ask` that nobody is there to answer is not an allow. The
            // kernel turns a `Decision::Ask` into a consent frame before it ever
            // gets here; by this point the question has been settled.
            Decision::Allow { .. } => PolicyDecision::Allow,
            Decision::Ask { rule, .. } => PolicyDecision::Deny {
                rule,
                reason: "this call needs consent, and none was given".to_owned(),
            },
            Decision::Deny { rule, reason } => PolicyDecision::Deny { rule, reason },
            // A verdict this crate has not been taught yet refuses, because a
            // gate that does not understand an answer has not been told yes.
            other => PolicyDecision::Deny {
                rule: other.rule(),
                reason: "the policy engine returned a verdict this gate does not know".to_owned(),
            },
        }
    }

    fn categorically_denies(&self, subject: &Subject, r#ref: &ToolRef) -> bool {
        let call = PendingCall::tool(r#ref.to_string());
        let explained = self.engine.explain(&call, subject);
        explained.verdict == orrery_policy::Verdict::Deny
    }
}


/// The aspects an extension is offered whether or not its manifest names one.
///
/// This is what the run path handed out unconditionally: `Capability::all` for
/// each of these four, for every extension, forever — which is why "an
/// extension denied `spawn` degrades" could not happen on the run path, because
/// `spawn` was never missing. They are still the *starting* set (a manifest
/// that asks for nothing is not an extension that may do nothing), and
/// [`grant_for`] is what takes one away again.
const BASELINE: &[Aspect] = &[Aspect::Tool, Aspect::Read, Aspect::Write, Aspect::Spawn];

/// What policy actually grants one extension, aspect by aspect.
///
/// **This is the phase-3 criterion, on the run path.** The loader hands this
/// grant to the host; the host disables every tool whose `requires` names an
/// aspect the grant does not carry
/// ([`orrery_host::host::disabled_by_grant`](../../orrery_host/host/fn.disabled_by_grant.html)),
/// and a load with a disabled tool is [`LoadOutcome::Degraded`] with the tool
/// named. So an extension whose `spawn` is refused loads, keeps every tool that
/// did not need `spawn`, and is *reported* — rather than failing, and rather
/// than being offered a tool it can never run.
///
/// # What is asked, and of whom
///
/// The subject is [`Subject::Ext`], so `[permissions."ext:<id>"]` narrows one
/// extension and the agent's own rules are inherited behind it — the same
/// subject chain a dispatch goes through, so a rule cannot mean one thing at
/// load and another at call time.
///
/// Each aspect is asked about the targets the manifest declared for it
/// (`spawn = ["*"]` asks about `spawn(*)`), or about `*` when it declared
/// none. **One refused target refuses the aspect**: a grant is coarse — it
/// carries an aspect or it does not — and pretending a partly-refused
/// capability is whole is how a tool comes to be offered and then denied at the
/// moment it is used.
///
/// An [`Decision::Ask`] counts as refused here for the same reason: there is
/// nobody to answer a question at load time, and the engine the harness builds
/// runs [`ConsentMode::Never`](orrery_policy::ConsentMode) anyway.
#[must_use]
pub fn grant_for(
    engine: &PolicyEngine,
    ext: &ExtId,
    wants: &[Capability],
    consent: Consent,
    scope: &AgentScope,
) -> Grant {
    let subject = Subject::Ext(ext.clone());
    let mut aspects: Vec<Aspect> = BASELINE.to_vec();
    for want in wants {
        if !aspects.contains(&want.aspect) {
            aspects.push(want.aspect);
        }
    }

    let mut capabilities = Vec::new();
    for aspect in aspects {
        let targets = declared_targets(wants, aspect);
        let granted = targets.iter().all(|target| {
            matches!(
                engine.check(&PendingCall::new(aspect, target.clone()), &subject, scope),
                Decision::Allow { .. }
            )
        });
        if granted {
            // `all`, not the manifest's narrower scope: the scope a call is
            // held to is the policy engine's, checked again at the moment of
            // the call. Narrowing here as well would be a second, coarser copy
            // of the same rules — and the day the two disagreed, the one
            // nobody could see would win.
            capabilities.push(Capability::all(aspect));
        }
    }
    Grant {
        capabilities,
        consent,
    }
}

/// What the manifest asked for under one aspect, or `*` when it asked for the
/// aspect without naming anything.
fn declared_targets(wants: &[Capability], aspect: Aspect) -> Vec<String> {
    let declared: Vec<String> = wants
        .iter()
        .filter(|c| c.aspect == aspect)
        .flat_map(|c| c.scope.clone())
        .collect();
    if declared.is_empty() {
        vec!["*".to_owned()]
    } else {
        declared
    }
}
