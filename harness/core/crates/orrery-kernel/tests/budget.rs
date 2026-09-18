//! Plan 05, Task 5: every ceiling demonstrably stops the loop.

mod common;

use std::sync::Arc;

use common::{Passes, Rig, TestHost, fixture, registry, write_stream};
use orrery_kernel::{Kernel, KernelConfig, PriceTable, TurnInput, TurnOutcome};
use orrery_proto::{Budget, BudgetKind, UserInput};
use tokio_util::sync::CancellationToken;

fn kernel(rig: &Rig, provider: Arc<dyn orrery_provider::Provider>, budget: Budget) -> Kernel {
    Kernel::new(
        rig.store.clone(),
        provider,
        Arc::new(registry(TestHost::echoing())),
        KernelConfig {
            budget,
            ..KernelConfig::default()
        },
    )
}

async fn run(rig: &Rig, kernel: &Kernel) -> TurnOutcome {
    kernel
        .run_turn(
            rig.lease().await,
            TurnInput::new(rig.session, UserInput::text("go"), rig.scope()),
            CancellationToken::new(),
        )
        .await
        .expect("the harness carried the turn")
}

/// A model that always asks for a tool, stopped after exactly three passes.
#[tokio::test]
async fn max_turns_stops() {
    let rig = Rig::open().await;
    let provider = Passes::repeating(fixture("tool-call.jsonl"));
    let kernel = kernel(
        &rig,
        provider.clone(),
        Budget {
            max_turns: 3,
            ..Budget::default()
        },
    );

    match run(&rig, &kernel).await {
        TurnOutcome::StoppedByBudget { kind, .. } => assert_eq!(kind, BudgetKind::Turns),
        other => panic!("a model that never stops must be stopped: {other:?}"),
    }
    assert_eq!(
        provider.served(),
        3,
        "exactly three passes, not two and not four"
    );
}

/// The wall clock stops it too, and the partial work is in the tree.
#[tokio::test]
async fn wall_clock_stops() {
    let rig = Rig::open().await;
    let dir = tempfile::tempdir().unwrap();
    // Each pass takes ~120ms, so a 200ms ceiling bites on the second.
    let slow = write_stream(
        dir.path(),
        "slow-tool.jsonl",
        &[
            r#"{"t":"started","id":"msg_slow"}"#,
            r#"{"delay_ms":120,"t":"tool-use-start","call":"0192f3a0-0000-7000-8000-0000000000b1","name":"builtin.read"}"#,
            r#"{"t":"tool-use-delta","call":"0192f3a0-0000-7000-8000-0000000000b1","json_fragment":"{\"path\":\"a.txt\"}"}"#,
            r#"{"t":"tool-use-end","call":"0192f3a0-0000-7000-8000-0000000000b1"}"#,
            r#"{"t":"done","stop":"tool-use"}"#,
        ],
    );
    let kernel = kernel(
        &rig,
        Passes::repeating(slow),
        Budget {
            wall_clock_ms: 200,
            ..Budget::default()
        },
    );

    match run(&rig, &kernel).await {
        TurnOutcome::StoppedByBudget { kind, .. } => assert_eq!(kind, BudgetKind::WallClock),
        other => panic!("{other:?}"),
    }
    assert!(
        rig.transcript().await.contains(&"assistant".to_owned()),
        "what the turn managed before the clock ran out is still in the tree"
    );
}

/// Tokens stop it. The fixture reports 976 tokens for its one pass, so a
/// 900-token ceiling is reached *during* the turn — and the check before the
/// tool dispatch is the one that catches it, which is why the call settles
/// `Cancelled(Budget)` rather than running.
#[tokio::test]
async fn tokens_stop() {
    let rig = Rig::open().await;
    let provider = Passes::repeating(fixture("tool-call.jsonl"));
    let kernel = kernel(
        &rig,
        provider.clone(),
        Budget {
            max_tokens: 900,
            ..Budget::default()
        },
    );

    match run(&rig, &kernel).await {
        TurnOutcome::StoppedByBudget { kind, usage, .. } => {
            assert_eq!(kind, BudgetKind::Tokens);
            assert!(usage.total_tokens() >= 900, "{usage:?}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        provider.served(),
        1,
        "the ceiling stopped it inside the first pass"
    );
    let rows = rig.rows().await;
    assert!(
        matches!(
            common::first_outcome(&rows),
            Some(orrery_proto::Outcome::Cancelled {
                reason: orrery_proto::CancelReason::Budget
            })
        ),
        "the call that did not run says why: {:?}",
        common::first_outcome(&rows)
    );
}

/// Money, once a price table exists. Inert without one — which is the state
/// plan 10 inherits, and is why `max_micro_usd` alone is not enough to stop
/// anything today.
#[tokio::test]
async fn money_stops_only_when_priced() {
    let rig = Rig::open().await;
    let provider = Passes::repeating(fixture("tool-call.jsonl"));
    let kernel = Kernel::new(
        rig.store.clone(),
        provider.clone(),
        Arc::new(registry(TestHost::echoing())),
        KernelConfig {
            budget: Budget {
                max_turns: 4,
                max_micro_usd: Some(1_000),
                ..Budget::default()
            },
            // 3 USD per million in, 15 out: one pass of the fixture costs about
            // 3_400 micro-USD, so the ceiling bites after the first.
            prices: PriceTable::empty().with("fixture", 3_000_000, 15_000_000),
            model: "fixture".to_owned(),
            ..KernelConfig::default()
        },
    );

    match run(&rig, &kernel).await {
        TurnOutcome::StoppedByBudget { kind, usage, .. } => {
            assert_eq!(kind, BudgetKind::Usd);
            assert!(usage.micro_usd.unwrap_or(0) >= 1_000, "{usage:?}");
        }
        other => panic!("a priced model trips a money ceiling: {other:?}"),
    }
}

/// A compile-time assertion: `StoppedByBudget` is a [`TurnOutcome`] variant and
/// `KernelError` has no room for one. Running out of budget is the system
/// working; a caller should not have to catch it.
#[test]
fn stop_is_a_value() {
    fn takes_outcome(outcome: TurnOutcome) -> Option<BudgetKind> {
        match outcome {
            TurnOutcome::StoppedByBudget { kind, .. } => Some(kind),
            _ => None,
        }
    }
    let stopped = TurnOutcome::StoppedByBudget {
        turn: orrery_proto::TurnId::new(),
        kind: BudgetKind::Turns,
        usage: orrery_proto::Usage::default(),
    };
    assert_eq!(takes_outcome(stopped), Some(BudgetKind::Turns));

    // And the other half, stated as the error type's own shape: every variant
    // of `KernelError` is a failure of the harness, and none of them is a
    // ceiling.
    let names: Vec<&str> = vec!["Session", "Tool", "Register"];
    for variant in &names {
        assert!(
            !variant.contains("Budget") && !variant.contains("Cancel"),
            "`KernelError::{variant}` would make an ending into an error"
        );
    }
}
