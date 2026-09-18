//! Ceilings that demonstrably trap — and a session that survives each one.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use orrery_host_wasm::{Ceilings, Outcome, WasmHost};
use orrery_tools::ToolBudget;

#[tokio::test(flavor = "multi_thread")]
async fn memory_ceiling_traps() {
    let host = WasmHost::new().expect("the engine builds");
    let component = host.compile(common::probe()).expect("the probe loads");

    let ceilings = Ceilings {
        memory_bytes: 8 * 1024 * 1024,
        wall_clock_ms: 20_000,
        instances: 16,
    };

    let outcome = host
        .call(
            &component,
            ceilings,
            Arc::new(common::Fake::denying()),
            host.cancel_handle(),
            "grow",
            "",
        )
        .await
        .expect("the call runs");

    match &outcome {
        Outcome::Trapped(why) => {
            assert!(
                why.contains("memory") || why.contains("abruptly"),
                "the reason should name the ceiling: {why}"
            );
        }
        other => panic!("a guest past its memory ceiling must trap: {other:?}"),
    }
    assert!(outcome.session_survives());

    // The session lives: the very next call on the same host works.
    let after = host
        .call(
            &component,
            Ceilings::DEFAULT,
            Arc::new(common::Fake::denying()),
            host.cancel_handle(),
            "echo",
            "still here",
        )
        .await
        .expect("the call runs");
    assert!(matches!(after, Outcome::Ok(_)), "{after:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn epoch_ceiling_traps() {
    let host = WasmHost::new().expect("the engine builds");
    let component = host.compile(common::probe()).expect("the probe loads");

    let ceilings = Ceilings {
        wall_clock_ms: 200,
        ..Ceilings::DEFAULT
    };

    let started = Instant::now();
    let outcome = host
        .call(
            &component,
            ceilings,
            Arc::new(common::Fake::denying()),
            host.cancel_handle(),
            "spin",
            "",
        )
        .await
        .expect("the call runs");
    let took = started.elapsed();

    match &outcome {
        Outcome::Trapped(why) => assert!(
            why.contains("wall-clock"),
            "the reason should name the ceiling: {why}"
        ),
        other => panic!("an infinite loop must be trapped: {other:?}"),
    }
    // Generous: the ticker's resolution is 10ms and the machine may be busy.
    // The claim is "within the budget", not "to the millisecond".
    assert!(
        took < Duration::from_secs(10),
        "the spin ran for {took:?}, which is not a wall-clock ceiling"
    );
}

/// A budget with no memory number is a wasm guest that nobody sized. It gets
/// the default ceiling, **not** no ceiling.
#[test]
fn a_missing_memory_budget_is_not_an_absent_ceiling() {
    let budget = ToolBudget::new(5_000, 1_000);
    assert_eq!(budget.memory_bytes, None);
    let ceilings = Ceilings::from_budget(&budget);
    assert_eq!(ceilings.memory_bytes, Ceilings::DEFAULT.memory_bytes);
    assert_eq!(ceilings.wall_clock_ms, 5_000);
}

#[test]
fn a_zero_wall_clock_budget_is_not_forever() {
    let budget = ToolBudget::new(0, 1_000);
    assert_eq!(
        Ceilings::from_budget(&budget).wall_clock_ms,
        Ceilings::DEFAULT.wall_clock_ms
    );
}
