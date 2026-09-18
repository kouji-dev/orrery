//! **The test that proves the boundary.**
//!
//! Everything else in this crate is a ceiling or a convenience. This file is
//! the claim the whole threat model rests on: a wasm extension cannot touch the
//! filesystem except through the broker.
//!
//! It is asserted from *inside* a guest, not from the host's configuration,
//! because "we did not call `preopened_dir`" is a statement about our code and
//! "the guest could not open a file" is a statement about the sandbox.

mod common;

use std::sync::Arc;

use orrery_host_wasm::{Ceilings, Outcome, WasmHost};

/// Every filesystem path a guest can think of fails, and there is no directory
/// for it to start from.
#[tokio::test(flavor = "multi_thread")]
async fn no_preopens() {
    let host = WasmHost::new().expect("the engine builds");
    let component = host.compile(common::probe()).expect("the probe loads");
    let broker = Arc::new(common::Fake::denying());

    let outcome = host
        .call(
            &component,
            Ceilings::DEFAULT,
            broker.clone(),
            host.cancel_handle(),
            "probe-fs",
            "",
        )
        .await
        .expect("the call runs");

    let Outcome::Ok(surface) = &outcome else {
        panic!("the probe should return, not trap: {outcome:?}");
    };
    let report = format!("{surface:?}");

    // The guest reports in capitals whenever something SUCCEEDED. Nothing may.
    for hole in ["READ ", "LISTED ", "WROTE "] {
        assert!(
            !report.contains(hole),
            "the sandbox has a hole — the guest managed `{hole}`: {report}"
        );
    }
    assert!(
        report.contains("no cwd"),
        "with no preopens there is no working directory to list: {report}"
    );
    assert!(
        report.contains("no write"),
        "with no preopens there is nowhere to write: {report}"
    );

    // And it did all that without asking the broker for anything, so this is
    // the guest failing to get out, not the broker refusing it.
    assert!(
        broker.calls().is_empty(),
        "the probe reached the broker: {:?}",
        broker.calls()
    );
}

/// The other half: the host never grants ambient authority in the first place.
///
/// A unit-level assertion, deliberately kept next to the guest-level one — if
/// somebody adds a `preopened_dir` call, this fails at the same time as the
/// test above and names the reason.
#[test]
fn the_wasi_context_is_built_with_nothing_in_it() {
    let source = include_str!("../src/store.rs");
    for granting in [
        "preopened_dir",
        "inherit_stdio",
        "inherit_env",
        "inherit_args",
        "inherit_network",
        "allow_tcp",
        "allow_udp",
        "allow_ip_name_lookup",
    ] {
        // The doc comment names each of these as something NOT called, so look
        // for a real call rather than a mention.
        assert!(
            !source.contains(&format!(".{granting}(")),
            "`store::no_preopens` calls `{granting}`, which opens a hole \
             straight past the policy engine"
        );
    }
}

/// A guest that only ever gets denials still runs to completion and still
/// returns a surface. Sandboxed does not mean broken.
#[tokio::test(flavor = "multi_thread")]
async fn a_guest_granted_nothing_still_runs() {
    let host = WasmHost::new().expect("the engine builds");
    let component = host.compile(common::probe()).expect("the probe loads");

    let outcome = host
        .call(
            &component,
            Ceilings::DEFAULT,
            Arc::new(common::Fake::denying()),
            host.cancel_handle(),
            "echo",
            "hello",
        )
        .await
        .expect("the call runs");

    match outcome {
        Outcome::Ok(surface) => {
            assert!(format!("{surface:?}").contains("hello"));
        }
        other => panic!("expected a surface: {other:?}"),
    }
}
