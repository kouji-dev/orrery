//! The five broker imports.
//!
//! The load-bearing one is the first: **a denial is a value here too.**

mod common;

use std::sync::Arc;

use orrery_host_wasm::broker::ProcOut;
use orrery_host_wasm::{Ceilings, Failure, Outcome, WasmHost};

async fn run(tool: &str, input: &str, broker: Arc<common::Fake>) -> Outcome {
    let host = WasmHost::new().expect("the engine builds");
    let component = host.compile(common::probe()).expect("the probe loads");
    host.call(
        &component,
        Ceilings::DEFAULT,
        broker,
        host.cancel_handle(),
        tool,
        input,
    )
    .await
    .expect("the call runs")
}

fn said(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Ok(surface) => format!("{surface:?}"),
        other => panic!("the guest should have returned a surface: {other:?}"),
    }
}

/// A guest with no `spawn` grant calling `run-proc` gets `error::denied` — and
/// **carries on**. It reports the denial in a surface, which is only possible
/// if it was still running afterwards.
#[tokio::test(flavor = "multi_thread")]
async fn denied_call_returns_error_not_trap() {
    let broker = Arc::new(common::Fake::denying());
    let outcome = run("run-proc", "", broker.clone()).await;

    let report = said(&outcome);
    assert!(
        report.contains("handled denied:"),
        "the guest must receive the denial as a value it can branch on: {report}"
    );
    assert!(
        report.contains("no `spawn` grant"),
        "the reason must survive the crossing: {report}"
    );
    assert_eq!(broker.calls(), vec!["run-proc echo hi"]);
}

/// Every import, not only `run-proc`. A denial that traps on one of five is a
/// denial that traps.
#[tokio::test(flavor = "multi_thread")]
async fn every_import_denies_as_a_value() {
    for (tool, input) in [
        ("run-proc", ""),
        ("read-file", "8"),
        ("fetch", "https://example.invalid/"),
        ("use-credential", "github"),
    ] {
        let outcome = run(tool, input, Arc::new(common::Fake::denying())).await;
        let report = said(&outcome);
        assert!(
            report.contains("handled denied:"),
            "`{tool}` did not deny as a value: {report}"
        );
    }
}

/// `read-file` truncates at `max_bytes` and says so. The flag travels with the
/// bytes — that is why the world returns a `file-out` record rather than a bare
/// `list<u8>`.
#[tokio::test(flavor = "multi_thread")]
async fn output_ceiling_applies() {
    let broker = Arc::new(common::Fake::holding(b"abcdefghijklmnopqrstuvwxyz"));
    let outcome = run("read-file", "8", broker.clone()).await;

    let report = said(&outcome);
    assert!(report.contains("read 8 bytes"), "{report}");
    assert!(report.contains("truncated=true"), "{report}");
    assert!(report.contains("abcdefgh"), "{report}");
    assert_eq!(broker.calls(), vec!["read-file fixture.txt max=8"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_short_file_is_not_flagged_truncated() {
    let broker = Arc::new(common::Fake::holding(b"abc"));
    let report = said(&run("read-file", "64", broker).await);
    assert!(report.contains("read 3 bytes"), "{report}");
    assert!(report.contains("truncated=false"), "{report}");
}

/// An allowed call works, and the broker's answer arrives intact.
#[tokio::test(flavor = "multi_thread")]
async fn an_allowed_call_returns_the_brokers_answer() {
    let broker = Arc::new(common::Fake {
        proc: Some(Ok(ProcOut {
            exit_code: 0,
            stdout: b"hi\n".to_vec(),
            stderr: Vec::new(),
            truncated: false,
        })),
        ..common::Fake::default()
    });
    let report = said(&run("run-proc", "", broker).await);
    assert!(report.contains("ran: exit=0"), "{report}");
    assert!(report.contains("stdout=hi"), "{report}");
}

/// A budget stop is a different value from a denial, and the guest can tell
/// them apart. Both are values; neither is a trap.
#[tokio::test(flavor = "multi_thread")]
async fn a_budget_stop_is_its_own_value() {
    let broker = Arc::new(common::Fake {
        proc: Some(Err(Failure::Budget("the turn's wall clock is spent".into()))),
        ..common::Fake::default()
    });
    let report = said(&run("run-proc", "", broker).await);
    assert!(report.contains("handled budget:"), "{report}");
}

/// **`imports::guest_never_sees_a_token`.**
///
/// A type-level check, not a runtime one. `HostBroker` is the whole surface the
/// guest's imports are served from, and `CapabilityToken` is not `Serialize`,
/// so it cannot be a WIT type. What is left to check is that nobody put one in
/// a signature anyway — so read the signatures.
#[test]
fn guest_never_sees_a_token() {
    // The ABI review note, kept as an executable assertion rather than a
    // comment in a doc nobody re-reads.
    let world = orrery_wit::WORLD;
    let import_block = world
        .split("interface broker {")
        .nth(1)
        .expect("the world has a broker interface");
    for line in import_block.lines() {
        let line = line.trim();
        if !line.contains(": func(") {
            continue;
        }
        let lower = line.to_lowercase();
        assert!(
            !lower.contains("token") && !lower.contains("capability"),
            "an import carries a token: {line}"
        );
    }

    // And the host trait, which is what those imports call, has none either.
    let trait_source = include_str!("../src/broker.rs");
    let in_trait = trait_source
        .split("pub trait HostBroker")
        .nth(1)
        .expect("the trait is there");
    let in_trait = in_trait.split("\n}").next().expect("the trait ends");
    assert!(
        !in_trait.contains("CapabilityToken") && !in_trait.contains("token:"),
        "HostBroker takes a token: the guest would then be able to hold one"
    );
}

/// A guest returning an arena that does not describe a surface is a trap, not a
/// panic and not a half-built tree. The arena came out of guest memory.
#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_arena_is_rejected() {
    let outcome = run("bad-arena", "", Arc::new(common::Fake::denying())).await;
    match &outcome {
        Outcome::Trapped(why) => assert!(why.contains("malformed surface"), "{why}"),
        other => panic!("a backward child index must be rejected: {other:?}"),
    }
}

/// The guest's own error is the message the model sees, and it is not a trap.
#[tokio::test(flavor = "multi_thread")]
async fn a_guest_error_is_a_message_not_a_trap() {
    let outcome = run("fail", "nope", Arc::new(common::Fake::denying())).await;
    match &outcome {
        Outcome::Failed(message) => assert!(message.contains("the guest refused: nope"), "{message}"),
        other => panic!("expected a failure message: {other:?}"),
    }
}
