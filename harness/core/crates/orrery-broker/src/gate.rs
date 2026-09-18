//! The join between the registry's dispatch and the policy engine.
//!
//! Plan 04 shipped `AllowAll` as the stub for step 4 of dispatch, with a note
//! that plan 07 substitutes the real thing. This is the substitution: it lives
//! here rather than in either crate because it is the only place that knows
//! both, and because a tool call reaching the outside world goes through this
//! crate anyway.
//!
//! It also does the load-time half — [`install`] — which is what makes an
//! extension denied `spawn` **degrade** instead of failing.

use std::sync::Arc;

use orrery_audit::{Audit, AuditEvent};
use orrery_policy::{Decision, PendingCall, PolicyEngine};
use orrery_proto::{
    Aspect, Contribution, ContributionKind, ExtId, Layer, LoadOutcome, Subject, ToolRef,
};
use orrery_tools::{CallCtx, PolicyCheck, PolicyDecision, Registry, ToolSpec};

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

/// What one tool an extension contributes needs in order to work.
#[derive(Clone, Debug)]
pub struct ToolNeeds {
    /// The tool, unqualified.
    pub spec: ToolSpec,
    /// What it must be granted, or it cannot be offered at all.
    pub requires: Vec<(Aspect, String)>,
}

impl ToolNeeds {
    /// A tool that needs nothing beyond being called.
    #[must_use]
    pub fn plain(name: &str) -> Self {
        Self {
            spec: ToolSpec::new(name),
            requires: Vec::new(),
        }
    }

    /// A tool that needs a capability.
    #[must_use]
    pub fn needing(name: &str, aspect: Aspect, target: impl Into<String>) -> Self {
        Self {
            spec: ToolSpec::new(name),
            requires: vec![(aspect, target.into())],
        }
    }
}

/// Install an extension, registering only the tools policy will let it use.
///
/// **This is the phase-3 criterion.** An extension whose `spawn` is denied does
/// not fail to load: it loads, the tool that needed `spawn` is left out, every
/// other tool it brought works, and the outcome says
/// [`LoadOutcome::Degraded`] with the problem named. Half an extension is
/// usable, and saying which half is what stops a user hunting for a tool that
/// quietly never registered.
///
/// Every decision — the denials and the grants — is in the audit with the rule
/// that produced it, because it went through [`PolicyEngine::check`].
#[must_use]
pub fn install(
    registry: &mut Registry,
    engine: &PolicyEngine,
    audit: &Audit,
    ext: &ExtId,
    layer: Layer,
    tools: &[ToolNeeds],
    scope: &orrery_proto::AgentScope,
) -> LoadOutcome {
    let subject = Subject::Ext(ext.clone());
    let mut contributions = Vec::new();
    let mut problems = Vec::new();

    for tool in tools {
        let mut refused: Option<String> = None;
        for (aspect, target) in &tool.requires {
            let call = PendingCall::new(*aspect, target.clone());
            match engine.check(&call, &subject, scope) {
                Decision::Allow { .. } => {}
                Decision::Ask { .. } => {
                    refused = Some(format!(
                        "`{}` needs `{}`, which requires consent that cannot be given at load",
                        tool.spec.name,
                        call.match_text()
                    ));
                }
                Decision::Deny { reason, .. } => {
                    refused = Some(format!(
                        "`{}` needs `{}`, which policy refuses: {reason}",
                        tool.spec.name,
                        call.match_text()
                    ));
                }
                _ => {
                    refused = Some(format!(
                        "`{}` needs `{}`, and the verdict was not one this loader knows",
                        tool.spec.name,
                        call.match_text()
                    ));
                }
            }
            if refused.is_some() {
                break;
            }
        }

        match refused {
            None => {
                registry.register(ext, layer, tool.spec.clone());
                contributions.push(Contribution {
                    kind: ContributionKind::Tool,
                    name: tool.spec.name.clone(),
                });
            }
            Some(problem) => problems.push(problem),
        }
    }

    let outcome = if problems.is_empty() {
        LoadOutcome::Ok {
            ext: ext.clone(),
            contributions,
            ms: 0,
        }
    } else {
        LoadOutcome::Degraded {
            ext: ext.clone(),
            contributions,
            ms: 0,
            problems,
        }
    };

    audit.append(AuditEvent::ExtensionLoad {
        ext: ext.clone(),
        status: match &outcome {
            LoadOutcome::Ok { .. } => "ok",
            LoadOutcome::Degraded { .. } => "degraded",
            LoadOutcome::Skipped { .. } => "skipped",
            LoadOutcome::Failed { .. } => "failed",
            _ => "unknown",
        }
        .to_owned(),
        contributions: outcome
            .contributions()
            .iter()
            .map(|c| c.name.clone())
            .collect(),
        problems: match &outcome {
            LoadOutcome::Degraded { problems, .. } => problems.clone(),
            _ => Vec::new(),
        },
    });

    outcome
}
