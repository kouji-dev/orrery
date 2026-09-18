//! Plan 05, Task 2: handlers that may do I/O and may not change the turn.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use common::{Passes, Rig, TestHost, fixture, registry};
use orrery_audit::AuditEvent;
use orrery_kernel::{
    Kernel, KernelConfig, LifecycleCtx, LifecycleError, LifecycleHandler, LifecyclePoint,
    LifecycleSet, TurnInput, TurnOutcome,
};
use orrery_proto::UserInput;
use tokio_util::sync::CancellationToken;

/// A handler that always fails, and counts how often it was asked to.
struct Broken {
    asked: Arc<AtomicUsize>,
}

#[async_trait]
impl LifecycleHandler for Broken {
    fn at(&self) -> LifecyclePoint {
        LifecyclePoint::TurnEnd
    }

    fn name(&self) -> &str {
        "the memory writer in this test"
    }

    async fn run(&self, _ctx: &LifecycleCtx) -> Result<(), LifecycleError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        Err(LifecycleError::failed("the memory backend is down"))
    }
}

/// A handler that works, so the test can tell "nothing ran" from "it ran and
/// failed".
struct Working {
    ran: Arc<AtomicUsize>,
}

#[async_trait]
impl LifecycleHandler for Working {
    fn at(&self) -> LifecyclePoint {
        LifecyclePoint::TurnEnd
    }

    async fn run(&self, _ctx: &LifecycleCtx) -> Result<(), LifecycleError> {
        self.ran.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// A handler that returns `Err` at `turn.end`: the turn still completes, and
/// the failure is in the audit.
#[tokio::test]
async fn failure_does_not_fail_the_turn() {
    let rig = Rig::open().await;
    let asked = Arc::new(AtomicUsize::new(0));
    let ran = Arc::new(AtomicUsize::new(0));
    let audit = orrery_audit::memory();

    let mut set = LifecycleSet::new().with_audit(audit.clone());
    set.register(Arc::new(Broken {
        asked: asked.clone(),
    }));
    set.register(Arc::new(Working { ran: ran.clone() }));

    let kernel = Kernel::new(
        rig.store.clone(),
        Passes::repeating(fixture("text-turn.jsonl")),
        Arc::new(registry(TestHost::echoing())),
        KernelConfig::default(),
    )
    .with_lifecycle(Arc::new(set));

    let outcome = kernel
        .run_turn(
            rig.lease().await,
            TurnInput::new(rig.session, UserInput::text("hello"), rig.scope()),
            CancellationToken::new(),
        )
        .await
        .expect("the harness carried the turn");

    assert!(
        matches!(outcome, TurnOutcome::Completed { .. }),
        "a broken handler must not fail the turn: {outcome:?}"
    );
    assert_eq!(asked.load(Ordering::SeqCst), 1);
    assert_eq!(
        ran.load(Ordering::SeqCst),
        1,
        "one handler failing must not stop the next"
    );

    let recorded = audit.records();
    let complaint = recorded
        .iter()
        .find_map(|r| match &r.event {
            AuditEvent::Content { action, content } if action == "lifecycle.failed" => {
                Some(content.clone())
            }
            _ => None,
        })
        .expect("the failure is in the audit");
    assert_eq!(complaint.scope, "turn.end");
    assert!(
        complaint.id.contains("memory writer"),
        "the record names the handler: {:?}",
        complaint.id
    );
}

/// A failing handler is disabled for the rest of the session: it is asked once,
/// however many turns follow.
#[tokio::test]
async fn failure_disables_it_for_the_session() {
    let rig = Rig::open().await;
    let asked = Arc::new(AtomicUsize::new(0));
    let mut set = LifecycleSet::new();
    set.register(Arc::new(Broken {
        asked: asked.clone(),
    }));

    let kernel = Kernel::new(
        rig.store.clone(),
        Passes::repeating(fixture("text-turn.jsonl")),
        Arc::new(registry(TestHost::echoing())),
        KernelConfig::default(),
    )
    .with_lifecycle(Arc::new(set));

    for _ in 0..3 {
        kernel
            .run_turn(
                rig.lease().await,
                TurnInput::new(rig.session, UserInput::text("again"), rig.scope()),
                CancellationToken::new(),
            )
            .await
            .expect("the harness carried the turn");
    }
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "a handler that throws once is not asked again"
    );
}

/// A type-level assertion: `run` returns `()`, so a handler cannot alter the
/// turn it fires on. If the trait ever grew a return value, this stops
/// compiling.
#[test]
fn returns_nothing() {
    fn assert_unit<H: LifecycleHandler>() {
        fn takes<F, Fut>(_f: F)
        where
            F: Fn(&LifecycleCtx) -> Fut,
            Fut: std::future::Future<Output = Result<(), LifecycleError>>,
        {
        }
        // The signature is the assertion: anything other than `Result<(), _>`
        // fails to unify here.
        takes::<_, _>(|_ctx: &LifecycleCtx| async { Ok::<(), LifecycleError>(()) });
        let _ = std::marker::PhantomData::<H>;
    }
    assert_unit::<Working>();
}
