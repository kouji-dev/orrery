//! Failure after load is as defined as failure during it.

mod common;

use std::sync::Arc;

use common::{TestExt, manifest, tool, tool_requiring};
use orrery_host::{ExtensionTable, NativeHost, NativeRegistry};
use orrery_proto::{Aspect, Capability, Consent, ExtId, Grant, Layer, LoadOutcome};

/// A grant of exactly these capabilities, asked every time.
fn grant(caps: Vec<Capability>) -> Grant {
    Grant {
        capabilities: caps,
        consent: Consent::Once,
    }
}

#[tokio::test]
async fn degraded_keeps_the_rest() {
    let src = manifest(
        "buildgraph",
        &["impacted", "deps"],
        "read = [\"$WORKSPACE/**\"]\nspawn = [\"java\"]",
    );
    let ext = Arc::new(TestExt::new(
        src,
        vec![tool_requiring("impacted", Aspect::Spawn), tool("deps")],
    ));
    let calls = ext.calls();

    let mut registry = NativeRegistry::new();
    registry.register(ext);
    let host = Arc::new(NativeHost::new(registry));
    let table = ExtensionTable::new();

    // `read` is granted; `spawn` is not.
    let outcome = table
        .load(
            host.clone(),
            host.manifest_of(&id("buildgraph")).unwrap(),
            Layer::Project,
            grant(vec![Capability::scoped(Aspect::Read, ["$WORKSPACE/**"])]),
        )
        .await;

    let problems = match &outcome {
        LoadOutcome::Degraded { problems, .. } => problems.clone(),
        other => panic!("expected Degraded, got {other:?}"),
    };
    assert!(
        problems.iter().any(|p| p.contains("spawn")),
        "the ledger says what is missing: {problems:?}"
    );

    let instance = table.get(&id("buildgraph")).unwrap();
    assert_eq!(instance.disabled(), ["impacted"]);
    assert!(
        instance.state().accepts_calls(),
        "half an extension is usable — that is the whole point of Degraded"
    );

    // The half that still works dispatches, through the registry, normally.
    let mut tools = orrery_tools::Registry::with_host(table.clone());
    table.register_into(&mut tools, &id("buildgraph"));
    assert!(tools.entry(&"buildgraph.deps".parse().unwrap()).is_some());
    assert!(
        tools
            .entry(&"buildgraph.impacted".parse().unwrap())
            .is_none(),
        "a tool that cannot work is not offered to the model at all"
    );

    let outcome = tools
        .dispatch(
            &"buildgraph.deps".parse().unwrap(),
            serde_json::json!({}),
            ctx_for("buildgraph"),
        )
        .await
        .unwrap();
    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(&*calls.lock(), &["deps"]);

    // And the disabled one refuses, by name, rather than misbehaving.
    let refusal = table
        .call_tool(
            &"buildgraph.impacted".parse().unwrap(),
            serde_json::json!({}),
            &ctx_for("buildgraph"),
        )
        .await
        .unwrap();
    match refusal {
        orrery_proto::Outcome::Denied { reason, .. } => {
            assert!(reason.contains("spawn"), "{reason}");
        }
        other => panic!("expected Denied, got {other:?}"),
    }
    assert_eq!(&*calls.lock(), &["deps"], "the disabled tool never ran");
}

#[tokio::test]
async fn singleton_conflict_resolves_by_layer() {
    let table = ExtensionTable::new();
    let mut registry = NativeRegistry::new();

    let far = "[extension]\napi = \"orrery-ext/1\"\nname = \"far\"\nversion = \"0.1.0\"\nruntime = \"native\"\n\n[provides]\nrouter = \"cheap-first\"\ntools = [\"pick\"]\n";
    let near = "[extension]\napi = \"orrery-ext/1\"\nname = \"near\"\nversion = \"0.1.0\"\nruntime = \"native\"\n\n[provides]\nrouter = \"project-router\"\ntools = [\"pick\"]\n";
    registry.register(Arc::new(TestExt::new(far, vec![tool("pick")])));
    registry.register(Arc::new(TestExt::new(near, vec![tool("pick")])));
    let host = Arc::new(NativeHost::new(registry));

    // The user-level one loads first and holds the slot.
    let first = table
        .load(
            host.clone(),
            host.manifest_of(&id("far")).unwrap(),
            Layer::User,
            grant(vec![]),
        )
        .await;
    assert!(matches!(first, LoadOutcome::Ok { .. }), "{first:?}");

    // Then the project-level one arrives. The closer layer wins.
    let second = table
        .load(
            host.clone(),
            host.manifest_of(&id("near")).unwrap(),
            Layer::Project,
            grant(vec![]),
        )
        .await;
    assert!(matches!(second, LoadOutcome::Ok { .. }), "{second:?}");
    assert_eq!(
        table.singleton_holder(orrery_ext_api::SingletonSlot::router),
        Some(id("near"))
    );

    // And the loser is named in the ledger, with the winner, so nobody has to
    // guess why their router is not being used.
    let loser = table.ledger().of(&id("far")).unwrap();
    match loser {
        LoadOutcome::Degraded {
            problems,
            contributions,
            ..
        } => {
            assert!(
                problems
                    .iter()
                    .any(|p| p.contains("router") && p.contains("near")),
                "{problems:?}"
            );
            assert!(
                !contributions
                    .iter()
                    .any(|c| c.kind == orrery_proto::ContributionKind::Router),
                "the loser no longer contributes the singleton"
            );
        }
        other => panic!("expected the loser to be Degraded, got {other:?}"),
    }

    // Its collection contributions are untouched: collections merge.
    let far = table.get(&id("far")).unwrap();
    assert!(far.state().accepts_calls());
    assert!(far.disabled().is_empty());
}

#[tokio::test]
async fn a_singleton_that_arrives_from_further_away_loses_at_once() {
    let table = ExtensionTable::new();
    let mut registry = NativeRegistry::new();
    for (name, layer) in [("near", Layer::Project), ("far", Layer::User)] {
        let _ = layer;
        registry.register(Arc::new(TestExt::new(
            format!(
                "[extension]\napi = \"orrery-ext/1\"\nname = \"{name}\"\nversion = \"0.1.0\"\nruntime = \"native\"\n\n[provides]\nmemory = \"{name}-memory\"\n"
            ),
            vec![],
        )));
    }
    let host = Arc::new(NativeHost::new(registry));

    table
        .load(
            host.clone(),
            host.manifest_of(&id("near")).unwrap(),
            Layer::Project,
            grant(vec![]),
        )
        .await;
    let late = table
        .load(
            host.clone(),
            host.manifest_of(&id("far")).unwrap(),
            Layer::User,
            grant(vec![]),
        )
        .await;

    match late {
        LoadOutcome::Degraded { problems, .. } => {
            assert!(
                problems.iter().any(|p| p.contains("memory")),
                "{problems:?}"
            );
        }
        other => panic!("expected Degraded, got {other:?}"),
    }
    assert_eq!(
        table.singleton_holder(orrery_ext_api::SingletonSlot::memory),
        Some(id("near"))
    );
}

#[tokio::test]
async fn collection_member_failure_disables_only_itself() {
    // The manifest promises three tools; the code contributes two. That is a
    // collection member failing, and it must cost exactly itself.
    let src = manifest("drifty", &["a", "b", "vanished"], "");
    let ext = Arc::new(TestExt::new(src, vec![tool("a"), tool("b")]));

    let mut registry = NativeRegistry::new();
    registry.register(ext);
    let host = Arc::new(NativeHost::new(registry));
    let table = ExtensionTable::new();

    let outcome = table
        .load(
            host.clone(),
            host.manifest_of(&id("drifty")).unwrap(),
            Layer::Project,
            grant(vec![]),
        )
        .await;

    match &outcome {
        LoadOutcome::Degraded {
            problems,
            contributions,
            ..
        } => {
            assert!(
                problems.iter().any(|p| p.contains("vanished")),
                "{problems:?}"
            );
            let names: Vec<&str> = contributions.iter().map(|c| c.name.as_str()).collect();
            assert_eq!(
                names,
                ["a", "b"],
                "only what actually exists is contributed"
            );
        }
        other => panic!("expected Degraded, got {other:?}"),
    }

    let mut tools = orrery_tools::Registry::with_host(table.clone());
    table.register_into(&mut tools, &id("drifty"));
    assert_eq!(tools.len(), 2);

    for name in ["drifty.a", "drifty.b"] {
        let outcome = tools
            .dispatch(
                &name.parse().unwrap(),
                serde_json::json!({}),
                ctx_for("drifty"),
            )
            .await
            .unwrap();
        assert!(outcome.is_ok(), "{name}: {outcome:?}");
    }
}

fn id(s: &str) -> ExtId {
    s.parse().unwrap()
}

/// A dispatch context that can see everything the extension offers.
fn ctx_for(ext: &str) -> orrery_tools::CallCtx {
    orrery_tools::CallCtx::new(
        orrery_proto::CallId::new(),
        orrery_proto::Subject::Agent,
        orrery_proto::AgentScope {
            agent: "main".into(),
            branch: orrery_proto::BranchId::new(),
            tools: vec![format!("{ext}.*")],
            grant: Grant::nothing(),
        },
        orrery_tools::ToolBudget::new(30_000, 1 << 20),
    )
}
