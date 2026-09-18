//! Cancellation, including the half that the epoch cannot do.

mod common;

use std::sync::Arc;
use std::time::Duration;

use orrery_host_wasm::{Ceilings, Outcome, WasmHost};

#[tokio::test(flavor = "multi_thread")]
async fn traps_a_spinning_guest() {
    let host = WasmHost::new().expect("the engine builds");
    let component = host.compile(common::probe()).expect("the probe loads");
    let cancel = host.cancel_handle();

    // A budget long enough that the epoch ceiling is not what stops it: if this
    // settles, cancellation is what did it.
    let ceilings = Ceilings {
        wall_clock_ms: 600_000,
        ..Ceilings::DEFAULT
    };

    let handle = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        handle.cancel();
    });

    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        host.call(
            &component,
            ceilings,
            Arc::new(common::Fake::denying()),
            cancel.clone(),
            "spin",
            "",
        ),
    )
    .await
    .expect("cancelling settles the call rather than hanging")
    .expect("the call runs");

    assert_eq!(outcome, Outcome::Cancelled, "{outcome:?}");
    assert!(cancel.is_cancelled());
    assert!(outcome.session_survives());
}

/// **Both halves.** A guest blocked inside `run-proc` is freed by the *broker*,
/// not by the epoch: the broker returns `cancelled`, the guest handles it, and
/// only then does the epoch trap it at its next backedge.
///
/// The witness is the `write-file` the guest makes between the two. If it is
/// there, the import returned a value and the guest ran on afterwards; if the
/// epoch alone had done the work, there would be nothing.
#[tokio::test(flavor = "multi_thread")]
async fn blocked_in_import_is_freed_by_the_broker() {
    let host = WasmHost::new().expect("the engine builds");
    let component = host.compile(common::probe()).expect("the probe loads");
    let cancel = host.cancel_handle();

    let gate = Arc::new(tokio::sync::Notify::new());
    let broker = Arc::new(common::Fake {
        block_until_cancelled: Some(Arc::clone(&gate)),
        ..common::Fake::default()
    });

    let ceilings = Ceilings {
        wall_clock_ms: 600_000,
        ..Ceilings::DEFAULT
    };

    let handle = cancel.clone();
    let broker_gate = Arc::clone(&gate);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        // The kernel does both, in this order: kill the brokered work, then
        // trap the guest. The first is what stops the work.
        broker_gate.notify_waiters();
        tokio::time::sleep(Duration::from_millis(100)).await;
        handle.cancel();
    });

    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        host.call(
            &component,
            ceilings,
            broker.clone(),
            cancel.clone(),
            "blocked-then-spin",
            "",
        ),
    )
    .await
    .expect("the call settles")
    .expect("the call runs");

    // Half one: the import returned `cancelled` as a value.
    let witness = broker
        .writes()
        .into_iter()
        .find(|(path, _)| path == "cancel-witness.txt");
    let (_, what) = witness.expect(
        "the guest never got past its import: the broker must free a blocked \
         guest with a value, or nothing else in this test can happen",
    );
    assert_eq!(
        String::from_utf8_lossy(&what),
        "cancelled",
        "the import must return `error::cancelled`"
    );

    // Half two: the guest then trapped, and the call settled cancelled.
    assert_eq!(outcome, Outcome::Cancelled, "{outcome:?}");
}

/// After cancellation, further imports refuse immediately rather than starting
/// work nobody wants.
#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_call_starts_no_more_work() {
    let host = WasmHost::new().expect("the engine builds");
    let component = host.compile(common::probe()).expect("the probe loads");
    let cancel = host.cancel_handle();
    cancel.cancel();

    let broker = Arc::new(common::Fake::holding(b"secret"));
    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        host.call(
            &component,
            Ceilings::DEFAULT,
            broker.clone(),
            cancel,
            "read-file",
            "8",
        ),
    )
    .await
    .expect("the call settles")
    .expect("the call runs");

    assert!(
        broker.calls().is_empty(),
        "a cancelled call reached the broker anyway: {:?}",
        broker.calls()
    );
    assert!(matches!(outcome, Outcome::Cancelled), "{outcome:?}");
}

/// The coarseness is readable as data, so a client can say the true thing
/// rather than implying a graceful stop.
#[test]
fn the_coarseness_is_stated_not_implied() {
    let host = WasmHost::new().expect("the engine builds");
    let truth = host.coarseness();
    assert!(!truth.guest_can_clean_up);
    assert!(truth.store_is_discarded);
    assert!(!truth.epoch_alone_frees_a_blocked_guest);
}
