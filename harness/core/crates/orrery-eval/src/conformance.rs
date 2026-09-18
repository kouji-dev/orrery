//! The singleton conformance suites, run as an eval suite.
//!
//! §4.14 makes a second, quieter promise: the same runner keeps the singleton
//! contracts honest. The suites themselves belong to the plans that own the
//! traits — [`orrery_session::conformance`] (plan 02) and
//! [`orrery_memory::conformance`] (plan 12) — and are *run* from here, so that
//! "does this alternative session backend actually work" is one command:
//!
//! ```bash
//! orrery eval run conformance
//! ```
//!
//! The permission-handler check is written here rather than in `orrery-policy`
//! for one reason: plan 07 enforces narrowing *mechanically*, inside
//! [`orrery_policy::review_narrowing`], which means a widening handler is
//! already harmless in production. What it is not is *visible*. This suite is
//! what makes a handler that tries to widen show up as a failing case with a
//! name, instead of a line in an audit log nobody reads.
//!
//! Every upstream suite panics on its first failure, because they are tests.
//! Each is therefore run inside `catch_unwind` and the panic becomes the case's
//! detail.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures_util::FutureExt as _;
use orrery_grader::EvalOutcome;
use orrery_memory::MemoryProvider;
use orrery_policy::{Decision, PermissionHandler, PendingCall, no_rule};
use orrery_proto::{
    Aspect, BranchId, ConsentPrompt, PromptId, SessionId, SessionRef, Subject, Surface,
    SurfaceKind, TextStyle,
};
use orrery_session::SessionStore;

use crate::report::{CostProvenance, EvalResult, RunReport, Timing};
use crate::run::Reproducibility;

/// The suite name `orrery eval run conformance` asks for.
pub const SUITE: &str = "conformance";

/// The singletons a conformance run checks.
///
/// Whatever is `None` is skipped rather than failed: a harness with no memory
/// provider bound is not a harness with a broken one.
#[derive(Clone, Default)]
pub struct Singletons {
    /// The bound session store (plan 02).
    pub session: Option<Arc<dyn SessionStore>>,
    /// The bound memory provider (plan 12).
    pub memory: Option<Arc<dyn MemoryProvider>>,
    /// The bound permission handler (plan 07).
    pub permissions: Option<Arc<dyn PermissionHandler>>,
}

impl Singletons {
    /// Nothing bound.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Check this session store.
    #[must_use]
    pub fn with_session(mut self, store: Arc<dyn SessionStore>) -> Self {
        self.session = Some(store);
        self
    }

    /// Check this memory provider.
    #[must_use]
    pub fn with_memory(mut self, provider: Arc<dyn MemoryProvider>) -> Self {
        self.memory = Some(provider);
        self
    }

    /// Check this permission handler.
    #[must_use]
    pub fn with_permissions(mut self, handler: Arc<dyn PermissionHandler>) -> Self {
        self.permissions = Some(handler);
        self
    }
}

/// Run every bound singleton's suite and report it like any other eval run.
pub async fn run_conformance(singletons: &Singletons) -> RunReport {
    let mut results = Vec::new();

    if let Some(store) = &singletons.session {
        let store = Arc::clone(store);
        results.push(
            case(
                "session-store",
                AssertUnwindSafe(orrery_session::conformance::run_conformance(store))
                    .catch_unwind()
                    .await
                    .map_err(panic_message),
            )
            .await,
        );
    }

    if let Some(provider) = &singletons.memory {
        let provider = Arc::clone(provider);
        results.push(
            case(
                "memory-provider",
                AssertUnwindSafe(orrery_memory::conformance::run_conformance(provider))
                    .catch_unwind()
                    .await
                    .map_err(panic_message),
            )
            .await,
        );
    }

    if let Some(handler) = &singletons.permissions {
        let outcome = AssertUnwindSafe(check_permission_handler(handler.as_ref()))
            .catch_unwind()
            .await
            .unwrap_or_else(|p| Err(panic_message(p)));
        results.push(case("permission-handler", outcome).await);
    }

    results.sort_by(|a, b| a.key().cmp(&b.key()));
    RunReport {
        run_id: format!("run-{}", uuid::Uuid::new_v4().simple()),
        suite: SUITE.to_owned(),
        // A conformance run touches no model, so nothing about it is
        // unpinned — the defaults are the truth here, not a claim.
        reproducibility: Reproducibility::default(),
        results,
    }
}

/// A handler that widens, and the message that says so.
///
/// The check is the contract in one line: for each proposed decision, ask the
/// handler and require a verdict no more permissive than the one it was shown.
/// `Allow → Ask | Deny` and `Ask → Deny` are fine; anything upward is not.
///
/// # Errors
///
/// A one-line description of the first widening, for the case's detail.
pub async fn check_permission_handler(handler: &dyn PermissionHandler) -> Result<(), String> {
    let call = PendingCall::read("$WORKSPACE/secrets.env");
    let subject = Subject::Agent;

    for proposed in [proposed_deny(), proposed_ask(&call)] {
        let before = proposed.verdict();
        let shown = format!("{before:?}");
        match handler.review(&call, &subject, proposed).await {
            // A handler that refuses to answer is denying, which is the safe
            // direction and conformant.
            Err(_) => continue,
            Ok(returned) => {
                if returned.verdict() > before {
                    return Err(format!(
                        "the handler widened {shown} to {:?} for `{}` — a handler may narrow \
                         (allow -> ask | deny, ask -> deny) and never widen",
                        returned.verdict(),
                        call.target,
                    ));
                }
            }
        }
    }
    Ok(())
}

fn proposed_deny() -> Decision {
    Decision::Deny {
        rule: no_rule(),
        reason: "conformance: nothing may read this".to_owned(),
    }
}

fn proposed_ask(call: &PendingCall) -> Decision {
    Decision::Ask {
        prompt: Box::new(ConsentPrompt {
            id: PromptId::new(),
            subject: Subject::Agent,
            capabilities: Vec::new(),
            reason: "conformance".to_owned(),
            rule: Some(no_rule()),
            surface: None,
        }),
        rule: no_rule(),
        fallback: Box::new(proposed_deny()),
        call: call.call,
        aspect: Aspect::Read,
        scope: orrery_policy::ResolvedScope::new(call.target.clone()),
    }
}

/// Turn one suite's result into a result row.
async fn case(name: &str, outcome: Result<(), String>) -> EvalResult {
    let (outcome, detail) = match outcome {
        Ok(()) => (
            EvalOutcome::Pass,
            Surface::new(SurfaceKind::Text {
                value: format!("{name} is conformant"),
                style: Some(TextStyle::Success),
            }),
        ),
        Err(message) => (
            EvalOutcome::Fail,
            Surface::new(SurfaceKind::Text {
                value: message,
                style: Some(TextStyle::Error),
            }),
        ),
    };
    EvalResult {
        case: name.to_owned(),
        profile: "bound".to_owned(),
        model: "none".to_owned(),
        seed: None,
        outcome,
        score: None,
        // Nothing was spent, and nothing estimated that: a conformance case
        // never reaches a provider.
        cost: orrery_proto::Usage::default(),
        cost_provenance: CostProvenance::MeasuredAtProviderBoundary,
        judge_cost: None,
        by_role: crate::report::ByRole::new(),
        timing: Timing::default(),
        turns: 0,
        tool_calls: 0,
        transcript: SessionRef {
            session: SessionId::new(),
            branch: BranchId::new(),
            turn: None,
        },
        detail: Some(detail),
    }
}

/// What a panicking suite was complaining about.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "the suite panicked".to_owned()
    }
}
