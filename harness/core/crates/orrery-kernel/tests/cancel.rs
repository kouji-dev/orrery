//! Plan 05, Task 7: one token tree, and what it leaves behind.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Passes, Rig, TestHost, fixture, registry, write_stream};
use orrery_kernel::{CallRevoker, Kernel, KernelConfig, TurnInput, TurnOutcome};
use orrery_proto::{Budget, CallId, CancelReason, Outcome, UserInput};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

/// Records which calls had their capabilities taken back.
#[derive(Default)]
struct Revoked(Mutex<Vec<CallId>>);

impl Revoked {
    fn calls(&self) -> Vec<CallId> {
        self.0.lock().clone()
    }
}

impl CallRevoker for Revoked {
    fn revoke(&self, call: CallId) {
        self.0.lock().push(call);
    }
}

/// Cancelling mid-stream: the turn reports `Cancelled(User)`, the partial
/// assistant text is in the tree, and the stream was dropped.
#[tokio::test]
async fn mid_stream() {
    let rig = Rig::open().await;
    // `never-ends.jsonl` emits one delta and then waits an hour. A correct
    // cancellation ends promptly; a broken one hangs the suite, which is the
    // point of the fixture.
    let provider = Passes::repeating(fixture("never-ends.jsonl"));
    let kernel = Kernel::new(
        rig.store.clone(),
        provider.clone(),
        Arc::new(registry(TestHost::echoing())),
        KernelConfig::default(),
    );

    let cancel = CancellationToken::new();
    let token = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(60)).await;
        token.cancel();
    });

    let started = std::time::Instant::now();
    let outcome = kernel
        .run_turn(
            rig.lease().await,
            TurnInput::new(rig.session, UserInput::text("go"), rig.scope()),
            cancel,
        )
        .await
        .expect("the harness carried the turn");

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "cancellation must not wait for the stream's own delay"
    );
    match outcome {
        TurnOutcome::Cancelled { reason, .. } => assert_eq!(reason, CancelReason::User),
        other => panic!("a cancelled turn is cancelled: {other:?}"),
    }

    let rows = rig.rows().await;
    let partial = rows
        .iter()
        .find_map(|r| match &r.kind {
            orrery_session::TurnKind::Assistant { content, .. } => Some(content.clone()),
            _ => None,
        })
        .expect("the partial answer is kept: it is what the person saw");
    assert!(
        format!("{partial:?}").contains("Thinking about it"),
        "{partial:?}"
    );
}

/// Cancelling during a dispatch: that call settles `Cancelled`, and its
/// capabilities are taken back so an in-flight tool cannot spend them.
#[tokio::test]
async fn mid_tool() {
    let rig = Rig::open().await;
    let host = TestHost::hanging();
    let revoked = Arc::new(Revoked::default());
    let kernel = Kernel::new(
        rig.store.clone(),
        Passes::repeating(fixture("tool-call.jsonl")),
        Arc::new(registry(host.clone())),
        KernelConfig::default(),
    )
    .with_revoker(revoked.clone());

    let cancel = CancellationToken::new();
    let token = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(80)).await;
        token.cancel();
    });

    let started = std::time::Instant::now();
    let outcome = kernel
        .run_turn(
            rig.lease().await,
            TurnInput::new(rig.session, UserInput::text("go"), rig.scope()),
            cancel,
        )
        .await
        .expect("the harness carried the turn");

    assert!(
        started.elapsed() < Duration::from_secs(5),
        "a hanging tool must not hold the turn open"
    );
    assert!(
        matches!(outcome, TurnOutcome::Cancelled { .. }),
        "{outcome:?}"
    );
    assert_eq!(
        host.recorder.len(),
        1,
        "the tool was reached, and then stopped"
    );

    let rows = rig.rows().await;
    match common::first_outcome(&rows).expect("the call settled") {
        Outcome::Cancelled { reason } => assert_eq!(*reason, CancelReason::User),
        other => panic!("a cancelled call settles cancelled: {other:?}"),
    }
    assert_eq!(
        revoked.calls().len(),
        1,
        "the call's capabilities were taken back, so an in-flight tool's next \
         broker call fails rather than succeeding on a grant nobody wants"
    );
}

/// A budget-triggered stop carries `CancelReason::Budget`, not `User`.
#[tokio::test]
async fn reason_is_recorded() {
    let rig = Rig::open().await;
    let provider = Passes::repeating(fixture("tool-call.jsonl"));
    let kernel = Kernel::new(
        rig.store.clone(),
        provider,
        Arc::new(registry(TestHost::echoing())),
        KernelConfig {
            // One pass's usage already exceeds this, so the ceiling is reached
            // between the stream and the dispatch.
            budget: Budget {
                max_tokens: 100,
                ..Budget::default()
            },
            ..KernelConfig::default()
        },
    );

    let outcome = kernel
        .run_turn(
            rig.lease().await,
            TurnInput::new(rig.session, UserInput::text("go"), rig.scope()),
            CancellationToken::new(),
        )
        .await
        .expect("the harness carried the turn");

    assert!(
        matches!(outcome, TurnOutcome::StoppedByBudget { .. }),
        "{outcome:?}"
    );
    let rows = rig.rows().await;
    match common::first_outcome(&rows).expect("the call that did not run settled") {
        Outcome::Cancelled { reason } => assert_eq!(
            *reason,
            CancelReason::Budget,
            "a budget stop must not be recorded as a person changing their mind"
        ),
        other => panic!("{other:?}"),
    }
}

/// A turn cancelled before it starts never reaches the provider.
#[tokio::test]
async fn cancelled_before_it_begins() {
    let rig = Rig::open().await;
    let dir = tempfile::tempdir().unwrap();
    let provider = Passes::repeating(write_stream(
        dir.path(),
        "text.jsonl",
        &[
            r#"{"t":"started","id":"m"}"#,
            r#"{"t":"text-delta","text":"hello"}"#,
            r#"{"t":"done","stop":"end-turn"}"#,
        ],
    ));
    let kernel = Kernel::new(
        rig.store.clone(),
        provider.clone(),
        Arc::new(registry(TestHost::echoing())),
        KernelConfig::default(),
    );

    let cancel = CancellationToken::new();
    cancel.cancel();
    let outcome = kernel
        .run_turn(
            rig.lease().await,
            TurnInput::new(rig.session, UserInput::text("go"), rig.scope()),
            cancel,
        )
        .await
        .expect("the harness carried the turn");

    assert!(
        matches!(outcome, TurnOutcome::Cancelled { .. }),
        "{outcome:?}"
    );
    assert_eq!(provider.served(), 0, "nothing was sent");
    assert_eq!(
        rig.transcript().await,
        vec!["user"],
        "the submitted turn is still recorded: it happened"
    );
}
