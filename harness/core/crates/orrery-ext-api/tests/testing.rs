//! The harness a community extension author gets, exercised the way they will
//! exercise it: no model, no network, no kernel.

use orrery_ext_api::testing::{BrokerCall, load_for_test};
use orrery_ext_api::{BrokerError, ListRequest, SpawnRequest};

const MANIFEST: &str = r#"
[extension]
api     = "orrery-ext/1"
name    = "buildgraph"
version = "1.2.0"
runtime = "native"

[provides]
tools = ["impacted"]

[requires]
read  = ["$WORKSPACE/**"]
spawn = ["java"]
"#;

#[tokio::test]
async fn denial_is_observable() {
    // The author declares the grants their test session hands out. `spawn` is
    // not among them, exactly as it would not be under a policy that refuses
    // it.
    let harness = load_for_test(MANIFEST, &["read:$WORKSPACE/**"]).unwrap();
    let ctx = harness.ctx("impacted");

    let err = ctx
        .broker
        .spawn(SpawnRequest::new("java", ["-jar", "bg.jar"]))
        .await
        .expect_err("no spawn grant, so no spawn");

    match &err {
        BrokerError::Denied { reason, .. } => {
            assert!(
                reason.contains("spawn"),
                "the denial names the aspect: {reason}"
            );
        }
        other => panic!("expected a denial, got {other:?}"),
    }

    // A denial is a value, and the author can see it in the outcome their tool
    // would return.
    assert!(matches!(
        err.into_outcome(),
        orrery_proto::Outcome::Denied { .. }
    ));

    // The important half: the mock *recorded* the attempt, so a test can assert
    // on what the extension tried to do, not only on what it got back.
    let recorded = harness.recorded();
    assert_eq!(recorded.len(), 1);
    match &recorded[0] {
        BrokerCall::Spawn {
            program,
            args,
            allowed,
        } => {
            assert_eq!(program, "java");
            assert_eq!(args, &["-jar", "bg.jar"]);
            assert!(!allowed);
        }
        other => panic!("expected a spawn call, got {other:?}"),
    }
}

#[tokio::test]
async fn a_granted_call_is_answered_from_the_canned_responses() {
    let harness = load_for_test(MANIFEST, &["read:$WORKSPACE/**", "spawn:java"])
        .unwrap()
        .with_spawn_response("java", "module-a\tchanged\n");
    let ctx = harness.ctx("impacted");

    let out = ctx
        .broker
        .spawn(SpawnRequest::new("java", ["-jar", "bg.jar"]))
        .await
        .expect("spawn was granted");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "module-a\tchanged\n");
    assert!(matches!(
        harness.recorded().first(),
        Some(BrokerCall::Spawn { allowed: true, .. })
    ));
}

#[tokio::test]
async fn the_ledger_is_the_one_a_real_session_produces() {
    let harness = load_for_test(MANIFEST, &["read:$WORKSPACE/**"]).unwrap();
    let outcome = harness.load_outcome();

    // `spawn` was asked for and not granted, so the author sees exactly the
    // `Degraded` a real session would show them, naming what is missing.
    match outcome {
        orrery_proto::LoadOutcome::Degraded { ext, problems, .. } => {
            assert_eq!(ext.as_str(), "buildgraph");
            assert!(problems.iter().any(|p| p.contains("spawn")), "{problems:?}");
        }
        other => panic!("expected Degraded, got {other:?}"),
    }
}

#[tokio::test]
async fn surfaces_come_back_as_data() {
    let harness = load_for_test(MANIFEST, &[]).unwrap();
    let ctx = harness.ctx("impacted");

    let surface = ctx.ui.table(["module", "reason"], [["a", "changed"]]);
    assert_eq!(harness.surfaces(), vec![surface]);
}

#[tokio::test]
async fn a_manifest_the_host_would_refuse_is_refused_here_too() {
    let src = MANIFEST.replace("orrery-ext/1", "orrery-ext/2");
    let err = load_for_test(&src, &[]).unwrap_err();
    assert_eq!(err.stage(), orrery_proto::LoadStage::Manifest);
}

/// Discovery is a broker call like any other, and the mock answers it the way a
/// real broker does: what the grants do not cover is not **named**.
#[tokio::test]
async fn a_listing_omits_what_the_grants_do_not_cover() {
    let harness = load_for_test(MANIFEST, &["read:$WORKSPACE/src/**"]).unwrap();
    let ctx = harness.ctx("impacted");
    harness.broker.add_file("$WORKSPACE/src/a.rs", "fn main() {}");
    harness.broker.add_file("$WORKSPACE/secret.txt", "shh");

    let listing = ctx
        .broker
        .list(ListRequest::new("$WORKSPACE", 100).recursive())
        .await
        .expect("listing is offered");

    let names: Vec<String> = listing
        .entries
        .iter()
        .map(|e| e.path.display().to_string().replace('\\', "/"))
        .collect();
    assert!(
        names.iter().any(|n| n.ends_with("src/a.rs")),
        "the granted file is there: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.ends_with("secret.txt")),
        "the ungranted one is not named at all: {names:?}"
    );
    assert!(matches!(
        harness.recorded().last(),
        Some(BrokerCall::List { recursive: true, .. })
    ));
}
