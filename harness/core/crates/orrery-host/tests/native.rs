//! `native` is a runtime, not a back door.

mod common;

use std::sync::Arc;

use common::{TestExt, manifest, tool};
use orrery_ext_api::ExtensionManifest;
use orrery_host::{ExtensionTable, NativeHost, NativeRegistry};
use orrery_proto::{
    Consent, ExtId, Grant, Layer, LoadOutcome, LoadStage, Outcome, RuleId, ToolRef,
};
use orrery_tools::{CallCtx, PolicyCheck, PolicyDecision, Registry};

/// A policy that says yes and remembers being asked.
#[derive(Default)]
struct Witness {
    seen: parking_lot::Mutex<Vec<String>>,
    deny: Option<String>,
}

impl PolicyCheck for Witness {
    fn check(&self, r#ref: &ToolRef, _input: &serde_json::Value, _ctx: &CallCtx) -> PolicyDecision {
        self.seen.lock().push(r#ref.to_string());
        match &self.deny {
            None => PolicyDecision::Allow,
            Some(reason) => PolicyDecision::Deny {
                rule: RuleId::new(),
                reason: reason.clone(),
            },
        }
    }
}

#[tokio::test]
async fn goes_through_dispatch() {
    let ext = Arc::new(TestExt::new(
        manifest("builtinish", &["read"], ""),
        vec![tool("read")],
    ));
    let calls = ext.calls();

    let mut natives = NativeRegistry::new();
    natives.register(ext);
    let host = Arc::new(NativeHost::new(natives));
    let table = ExtensionTable::new();
    table
        .load(
            host.clone(),
            host.manifest_of(&id("builtinish")).unwrap(),
            Layer::Project,
            Grant {
                capabilities: vec![],
                consent: Consent::Always,
            },
        )
        .await;

    let witness = Arc::new(Witness::default());
    let mut registry = Registry::with_host(table.clone()).with_policy(witness.clone());
    table.register_into(&mut registry, &id("builtinish"));

    let r#ref: ToolRef = "builtinish.read".parse().unwrap();
    let outcome = registry
        .dispatch(&r#ref, serde_json::json!({ "path": "x" }), ctx())
        .await
        .unwrap();

    assert!(outcome.is_ok(), "{outcome:?}");
    assert_eq!(
        &*witness.seen.lock(),
        &["builtinish.read"],
        "a compiled-in extension is policy-checked like any other"
    );
    assert_eq!(&*calls.lock(), &["read"]);
}

#[tokio::test]
async fn a_denied_native_call_never_reaches_the_extension() {
    let ext = Arc::new(TestExt::new(
        manifest("builtinish", &["read"], ""),
        vec![tool("read")],
    ));
    let calls = ext.calls();

    let mut natives = NativeRegistry::new();
    natives.register(ext);
    let host = Arc::new(NativeHost::new(natives));
    let table = ExtensionTable::new();
    table
        .load(
            host.clone(),
            host.manifest_of(&id("builtinish")).unwrap(),
            Layer::Project,
            Grant::nothing(),
        )
        .await;

    let witness = Arc::new(Witness {
        seen: parking_lot::Mutex::default(),
        deny: Some("no reading today".to_owned()),
    });
    let mut registry = Registry::with_host(table.clone()).with_policy(witness);
    table.register_into(&mut registry, &id("builtinish"));

    let outcome = registry
        .dispatch(
            &"builtinish.read".parse().unwrap(),
            serde_json::json!({}),
            ctx(),
        )
        .await
        .unwrap();

    assert!(matches!(outcome, Outcome::Denied { .. }), "{outcome:?}");
    assert!(
        calls.lock().is_empty(),
        "the only difference from a community extension is where the code was \
         compiled — not whether policy runs first"
    );
}

#[tokio::test]
async fn manifest_is_the_same_shape() {
    // The builtin bundle's own manifest, parsed by the same parser a
    // third-party one goes through. No second reader, no relaxed rules.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../extensions/crates/orrery-ext-tools-builtin/orrery.toml");
    let shipped = ExtensionManifest::from_path(&path).expect("the builtin bundle parses");
    assert_eq!(shipped.name.as_str(), "builtin");
    assert_eq!(shipped.runtime, orrery_ext_api::RuntimeKind::Native);
    assert!(shipped.provides.tools.contains(&"read".to_owned()));

    // And a native extension carrying that manifest loads through the same
    // table as anything else.
    let src = std::fs::read_to_string(&path).unwrap();
    let mut natives = NativeRegistry::new();
    natives.register(Arc::new(TestExt::new(src, vec![tool("read")])));
    let host = Arc::new(NativeHost::new(natives));
    assert!(host.manifest_of(&id("builtin")).is_some());
}

#[tokio::test]
async fn an_unparseable_manifest_fails_at_the_manifest_stage() {
    let mut natives = NativeRegistry::new();
    natives.register(Arc::new(TestExt::new("not toml {{{", vec![])));
    assert_eq!(natives.broken().len(), 1);
    assert_eq!(natives.broken()[0].stage(), LoadStage::Manifest);
}

#[tokio::test]
async fn an_api_major_we_do_not_know_is_skipped_not_half_loaded() {
    let src = manifest("future", &["x"], "").replace("orrery-ext/1", "orrery-ext/7");
    let mut natives = NativeRegistry::new();
    natives.register(Arc::new(TestExt::new(src, vec![tool("x")])));
    assert!(
        natives
            .broken()
            .iter()
            .any(|e| e.to_string().contains("orrery-ext/7"))
    );
    assert!(
        natives.manifest_of(&id("future")).is_none(),
        "it is not in the table at all"
    );
}

#[tokio::test]
async fn a_native_extension_that_is_not_registered_fails_to_load() {
    let host = Arc::new(NativeHost::new(NativeRegistry::new()));
    let manifest = Arc::new(
        ExtensionManifest::from_toml_str(&manifest("ghost", &["x"], ""), "ghost/orrery.toml")
            .unwrap(),
    );
    let table = ExtensionTable::new();
    let outcome = table
        .load(host, manifest, Layer::Project, Grant::nothing())
        .await;
    match outcome {
        LoadOutcome::Failed { stage, .. } => assert_eq!(stage, LoadStage::Link),
        other => panic!("expected Failed at Link, got {other:?}"),
    }
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
        orrery_tools::ToolBudget::new(30_000, 1 << 20),
    )
}
