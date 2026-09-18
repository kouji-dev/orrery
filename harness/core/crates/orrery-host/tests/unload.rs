//! The phase-2 acceptance criterion: killing one extension leaves the session
//! alive.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Mode, TestExt, manifest, tool};
use orrery_host::{ExtensionTable, NativeHost, NativeRegistry};
use orrery_proto::{ExtId, Grant, Layer, Outcome};
use orrery_tools::{CallCtx, Registry, ToolBudget};

async fn two_searchers() -> (Arc<ExtensionTable>, Registry) {
    let mut natives = NativeRegistry::new();
    for name in ["alpha", "beta"] {
        natives.register(Arc::new(TestExt::new(
            manifest(name, &["search"], ""),
            vec![tool("search")],
        )));
    }
    let host = Arc::new(NativeHost::new(natives));
    let table = ExtensionTable::new();

    let mut registry = Registry::with_host(table.clone());
    for (name, layer) in [("alpha", Layer::User), ("beta", Layer::Project)] {
        table
            .load(
                host.clone(),
                host.manifest_of(&id(name)).unwrap(),
                layer,
                Grant::nothing(),
            )
            .await;
        table.register_into(&mut registry, &id(name));
    }
    (table, registry)
}

#[tokio::test]
async fn killing_one_leaves_the_session_alive() {
    let (table, registry) = two_searchers().await;

    // Both claim `search`, and both work: ambiguity is a value, not an error.
    assert!(registry.entry(&"alpha.search".parse().unwrap()).is_some());
    assert!(registry.entry(&"beta.search".parse().unwrap()).is_some());

    // A reference resolved *before* the unload. This is the stale one.
    let stale = "alpha.search".parse().unwrap();
    let before = registry
        .dispatch(&stale, serde_json::json!({}), ctx())
        .await
        .expect("no turn failed");
    assert!(before.is_ok());

    table
        .unload(&id("alpha"), Duration::from_secs(1))
        .await
        .unwrap();

    // The other one still dispatches.
    let other = registry
        .dispatch(
            &"beta.search".parse().unwrap(),
            serde_json::json!({}),
            ctx(),
        )
        .await
        .expect("no turn failed");
    assert!(other.is_ok(), "{other:?}");

    // The stale reference settles `Unloaded` — a tool outcome the model can
    // re-plan around, and emphatically not an `Err`.
    let after = registry
        .dispatch(&stale, serde_json::json!({}), ctx())
        .await
        .expect("a stale ref must not fail the turn");
    match after {
        Outcome::Unloaded { ext } => assert_eq!(ext, id("alpha")),
        other => panic!("expected Unloaded, got {other:?}"),
    }

    assert!(table.get(&id("alpha")).is_none());
    assert!(table.get(&id("beta")).is_some());
}

#[tokio::test]
async fn a_new_generation_does_not_answer_for_the_old_one() {
    let mut natives = NativeRegistry::new();
    natives.register(Arc::new(TestExt::new(
        manifest("alpha", &["search"], ""),
        vec![tool("search")],
    )));
    let host = Arc::new(NativeHost::new(natives));
    let table = ExtensionTable::new();

    let m = host.manifest_of(&id("alpha")).unwrap();
    table
        .load(host.clone(), m.clone(), Layer::Project, Grant::nothing())
        .await;
    let first = table.get(&id("alpha")).unwrap().generation();

    table
        .unload(&id("alpha"), Duration::from_millis(100))
        .await
        .unwrap();
    table
        .load(host.clone(), m, Layer::Project, Grant::nothing())
        .await;
    let second = table.get(&id("alpha")).unwrap().generation();

    assert!(second > first, "{second} must be past {first}");
    assert!(
        table.resolve(&id("alpha"), first).is_none(),
        "a reference held across the reload cannot resurrect the old instance"
    );
    assert!(table.resolve(&id("alpha"), second).is_some());
}

#[tokio::test]
async fn in_flight_calls_settle_cancelled() {
    let mut natives = NativeRegistry::new();
    natives.register(Arc::new(
        TestExt::new(manifest("slow", &["wait"], ""), vec![tool("wait")])
            .with_mode(Mode::AwaitCancel),
    ));
    let host = Arc::new(NativeHost::new(natives));
    let table = ExtensionTable::new();
    table
        .load(
            host.clone(),
            host.manifest_of(&id("slow")).unwrap(),
            Layer::Project,
            Grant::nothing(),
        )
        .await;

    let calling = {
        let table = table.clone();
        tokio::spawn(async move {
            table
                .call_tool(&"slow.wait".parse().unwrap(), serde_json::json!({}), &ctx())
                .await
        })
    };

    // Let the call actually get in flight before pulling the rug.
    tokio::time::sleep(Duration::from_millis(50)).await;
    table
        .unload(&id("slow"), Duration::from_secs(2))
        .await
        .unwrap();

    let settled = calling.await.unwrap().unwrap();
    assert!(
        matches!(settled, Outcome::Cancelled { .. }),
        "an in-flight call settles Cancelled, it does not vanish: {settled:?}"
    );
}

#[tokio::test]
async fn grace_then_kill() {
    let mut natives = NativeRegistry::new();
    natives.register(Arc::new(
        TestExt::new(manifest("stubborn", &["spin"], ""), vec![tool("spin")])
            .with_mode(Mode::IgnoreCancel),
    ));
    let host = Arc::new(NativeHost::new(natives));
    let table = ExtensionTable::new();
    table
        .load(
            host.clone(),
            host.manifest_of(&id("stubborn")).unwrap(),
            Layer::Project,
            Grant::nothing(),
        )
        .await;

    let _calling = {
        let table = table.clone();
        tokio::spawn(async move {
            table
                .call_tool(
                    &"stubborn.spin".parse().unwrap(),
                    serde_json::json!({}),
                    &ctx(),
                )
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(50)).await;

    let started = Instant::now();
    table
        .unload(&id("stubborn"), Duration::from_millis(150))
        .await
        .unwrap();
    let took = started.elapsed();

    assert!(
        took < Duration::from_secs(2),
        "a child that ignores the cancel is killed after the grace window, not \
         waited on forever (took {took:?})"
    );
    assert!(table.get(&id("stubborn")).is_none());
    assert_eq!(
        table
            .call_tool(
                &"stubborn.spin".parse().unwrap(),
                serde_json::json!({}),
                &ctx()
            )
            .await
            .unwrap(),
        Outcome::Unloaded {
            ext: id("stubborn")
        }
    );
}

#[tokio::test]
async fn unloading_something_that_is_not_there_is_not_a_failure() {
    let table = ExtensionTable::new();
    assert!(
        table
            .unload(&id("nobody"), Duration::from_millis(10))
            .await
            .is_err(),
        "the caller asked for something impossible, and is told so"
    );
}

fn id(s: &str) -> ExtId {
    s.parse().unwrap()
}

fn ctx() -> CallCtx {
    CallCtx::new(
        orrery_proto::CallId::new(),
        orrery_proto::Subject::Agent,
        orrery_proto::AgentScope {
            agent: "main".into(),
            branch: orrery_proto::BranchId::new(),
            tools: vec!["*".into()],
            grant: Grant::nothing(),
        },
        ToolBudget::new(30_000, 1 << 20),
    )
}
