//! Task 8: the singleton conformance suites, run as `orrery eval run
//! conformance` runs them.

mod common;

use std::sync::Arc;

use async_trait::async_trait;
use common::store::MemoryStore;
use orrery_eval::conformance::{SUITE, Singletons, check_permission_handler, run_conformance};
use orrery_eval::EvalOutcome;
use orrery_policy::{
    Decision, HandlerError, PendingCall, PermissionHandler, ResolvedScope, Verdict, no_rule,
};
use orrery_proto::{ConsentPrompt, PromptId, Subject};
use orrery_session::SessionStore;

/// Narrows, as a handler must: everything becomes a deny.
struct AlwaysDenies;

#[async_trait]
impl PermissionHandler for AlwaysDenies {
    async fn review(
        &self,
        _call: &PendingCall,
        _subject: &Subject,
        _proposed: Decision,
    ) -> Result<Decision, HandlerError> {
        Ok(Decision::Deny {
            rule: no_rule(),
            reason: "no".to_owned(),
        })
    }
}

/// Deliberately broken: it turns a deny into an ask.
///
/// It cannot return an `Allow`, because minting a [`CapabilityToken`] is
/// private to `orrery-policy` — which is plan 07 working as designed. `Deny ->
/// Ask` is a widening all the same, and it is the one a real handler is most
/// likely to attempt: "let me put this to the user" over a rule that already
/// said no.
struct Widens;

#[async_trait]
impl PermissionHandler for Widens {
    async fn review(
        &self,
        call: &PendingCall,
        _subject: &Subject,
        _proposed: Decision,
    ) -> Result<Decision, HandlerError> {
        Ok(Decision::Ask {
            prompt: Box::new(ConsentPrompt {
                id: PromptId::new(),
                subject: Subject::Agent,
                capabilities: Vec::new(),
                reason: "let me ask the user".to_owned(),
                rule: Some(no_rule()),
                surface: None,
            }),
            rule: no_rule(),
            fallback: Box::new(Decision::Deny {
                rule: no_rule(),
                reason: "nobody answered".to_owned(),
            }),
            call: call.call,
            aspect: call.aspect,
            scope: ResolvedScope::new(call.target.clone()),
        })
    }
}

/// Cannot answer at all. Denying is the safe direction, so this is conformant.
struct Refuses;

#[async_trait]
impl PermissionHandler for Refuses {
    async fn review(
        &self,
        _call: &PendingCall,
        _subject: &Subject,
        _proposed: Decision,
    ) -> Result<Decision, HandlerError> {
        Err(HandlerError::Panicked)
    }
}

#[tokio::test]
async fn detects_a_widening_permission_handler() {
    let report = run_conformance(&Singletons::new().with_permissions(Arc::new(Widens))).await;

    assert_eq!(report.suite, SUITE);
    let result = report
        .result("permission-handler", "bound")
        .expect("the handler was checked");
    assert_eq!(result.outcome, EvalOutcome::Fail);

    let message = format!("{:?}", result.detail);
    assert!(
        message.contains("widened"),
        "the failure must say what went wrong: {message}"
    );
    assert!(
        message.contains("Deny") && message.contains("Ask"),
        "and in which direction: {message}"
    );
}

#[tokio::test]
async fn a_narrowing_handler_conforms() {
    let report = run_conformance(&Singletons::new().with_permissions(Arc::new(AlwaysDenies))).await;
    assert_eq!(
        report
            .result("permission-handler", "bound")
            .expect("checked")
            .outcome,
        EvalOutcome::Pass
    );
}

#[tokio::test]
async fn a_handler_that_cannot_answer_conforms() {
    // Refusing to answer denies the call, which is narrowing, not widening.
    assert!(check_permission_handler(&Refuses).await.is_ok());
}

#[tokio::test]
async fn the_bound_session_store_is_checked() {
    let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::default());
    let report = run_conformance(&Singletons::new().with_session(store)).await;

    let result = report
        .result("session-store", "bound")
        .expect("the store was checked");
    assert_eq!(
        result.outcome,
        EvalOutcome::Pass,
        "plan 02's own suite should pass against the reference store: {:?}",
        result.detail
    );
    assert_eq!(
        report.results.len(),
        1,
        "nothing else was bound, and an unbound singleton is skipped, not failed"
    );
}

#[tokio::test]
async fn a_broken_session_store_fails_with_a_message() {
    let report = run_conformance(&Singletons::new().with_session(Arc::new(Hopeless))).await;
    let result = report.result("session-store", "bound").expect("checked");
    assert_eq!(result.outcome, EvalOutcome::Fail);
    assert_eq!(
        report.passed(),
        0,
        "the run reports the failure rather than panicking out of the suite"
    );
    assert!(
        result.detail.is_some(),
        "and it says what the suite was complaining about"
    );
}

#[tokio::test]
async fn nothing_bound_is_an_empty_run_not_a_failure() {
    let report = run_conformance(&Singletons::new()).await;
    assert!(report.results.is_empty());
    assert_eq!(report.passed(), 0);
}

/// A session store that refuses everything. Stands in for a backend that does
/// not implement the contract.
struct Hopeless;

#[async_trait]
impl SessionStore for Hopeless {
    async fn create(
        &self,
        _workspace: &str,
        _profile: &str,
    ) -> Result<orrery_proto::SessionId, orrery_session::SessionError> {
        Err(orrery_session::SessionError::Backend {
            detail: "this backend does nothing".to_owned(),
        })
    }

    async fn open(
        &self,
        session: orrery_proto::SessionId,
    ) -> Result<orrery_session::turn::SessionHandle, orrery_session::SessionError> {
        Err(orrery_session::SessionError::NoSuchSession { session })
    }

    async fn lease(
        &self,
        branch: orrery_proto::BranchId,
    ) -> Result<orrery_session::BranchLease, orrery_session::SessionError> {
        Err(orrery_session::SessionError::NoSuchBranch { branch })
    }

    async fn append(
        &self,
        _lease: &orrery_session::BranchLease,
        _turn: orrery_session::turn::NewTurn,
    ) -> Result<orrery_proto::TurnId, orrery_session::SessionError> {
        Err(orrery_session::SessionError::Backend {
            detail: "no".to_owned(),
        })
    }

    async fn branch(
        &self,
        from: orrery_proto::TurnId,
        _label: &str,
    ) -> Result<orrery_proto::BranchId, orrery_session::SessionError> {
        Err(orrery_session::SessionError::NoSuchTurn { turn: from })
    }

    async fn close_branch(
        &self,
        _lease: orrery_session::BranchLease,
        _outcome: orrery_session::turn::BranchOutcome,
    ) -> Result<(), orrery_session::SessionError> {
        Ok(())
    }

    async fn materialise(
        &self,
        branch: orrery_proto::BranchId,
        _budget: orrery_proto::TokenBudget,
        _counter: &dyn orrery_session::algebra::TokenCounter,
    ) -> Result<orrery_session::algebra::Materialised, orrery_session::SessionError> {
        Err(orrery_session::SessionError::NoSuchBranch { branch })
    }

    async fn compact(
        &self,
        _lease: &orrery_session::BranchLease,
        _upto: orrery_proto::Seq,
        _summary: orrery_session::turn::NewTurn,
    ) -> Result<orrery_session::turn::CompactResult, orrery_session::SessionError> {
        Err(orrery_session::SessionError::Backend {
            detail: "no".to_owned(),
        })
    }

    async fn events_since(
        &self,
        session: orrery_proto::SessionId,
        _since: Option<orrery_proto::Seq>,
    ) -> Result<Vec<orrery_session::turn::StoredEvent>, orrery_session::SessionError> {
        Err(orrery_session::SessionError::NoSuchSession { session })
    }
}

#[test]
fn the_verdict_ordering_is_the_one_narrowing_runs_under() {
    // The check in `check_permission_handler` is `returned.verdict() >
    // proposed.verdict()`, which only means anything under this ordering.
    assert!(Verdict::Deny < Verdict::Ask);
    assert!(Verdict::Ask < Verdict::Allow);
}

#[tokio::test]
async fn the_bound_memory_provider_is_checked() {
    // Plan 12's suite, over plan 12's own in-test provider: `orrery eval run
    // conformance` is how an alternative memory provider gets the same
    // treatment.
    let provider = Arc::new(orrery_memory::testing::InMemoryProvider::anything("in-memory"));
    let report = run_conformance(&Singletons::new().with_memory(provider)).await;

    let result = report
        .result("memory-provider", "bound")
        .expect("the provider was checked");
    assert_eq!(
        result.outcome,
        EvalOutcome::Pass,
        "the reference provider should satisfy the scope lifetimes: {:?}",
        result.detail
    );
}
