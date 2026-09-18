//! Task 4 · layers and subjects.

mod common;

use std::sync::Arc;

use common::{wide_scope, Workspace};
use orrery_policy::{PendingCall, PolicyBuilder, PolicyEngine, Verdict};
use orrery_proto::{ExtId, Layer, Subject};

fn engine_of(ws: &Workspace, layers: &[(Layer, &str)]) -> PolicyEngine {
    let mut builder = PolicyBuilder::new(ws.root());
    for (layer, toml) in layers {
        builder = builder
            .layer_toml(toml, format!("{layer:?}.toml"), *layer, false)
            .expect("the fixture parses");
    }
    PolicyEngine::new(builder.build().expect("it compiles"))
}

fn verdict(engine: &PolicyEngine, call: PendingCall, subject: &Subject) -> Verdict {
    engine.check(&call, subject, &wide_scope()).verdict()
}

/// A user allow cannot relax a managed deny. Not by precedence — by evaluation
/// order: every deny from every layer is walked before any allow is looked at.
#[test]
fn managed_deny_is_final() {
    let ws = Workspace::new();
    let engine = engine_of(
        &ws,
        &[
            (Layer::Managed, "[permissions]\ndeny = [\"net(domain: *)\"]\n"),
            (
                Layer::Project,
                "[permissions]\nallow = [\"net(domain: *.corp.internal)\"]\n",
            ),
        ],
    );
    assert_eq!(
        verdict(&engine, PendingCall::net("build.corp.internal"), &Subject::Agent),
        Verdict::Deny,
        "the closest layer wins for a NAME, never for a permission"
    );
}

/// Denies from three layers all apply: deny is a union, not a precedence.
#[test]
fn deny_is_a_union() {
    let ws = Workspace::new();
    let engine = engine_of(
        &ws,
        &[
            (Layer::Managed, "[permissions]\ndeny = [\"creds(*)\"]\n"),
            (Layer::User, "[permissions]\ndeny = [\"tool(shell.*)\"]\n"),
            (
                Layer::Project,
                "[permissions]\ndeny = [\"net(domain: *)\"]\nallow = [\"tool(*)\", \"creds(*)\", \"net(domain: *)\"]\n",
            ),
        ],
    );
    for call in [
        PendingCall::creds("ANTHROPIC_API_KEY"),
        PendingCall::tool("shell.exec"),
        PendingCall::net("example.com"),
    ] {
        assert_eq!(
            verdict(&engine, call.clone(), &Subject::Agent),
            Verdict::Deny,
            "{call} should be denied by one of the three layers"
        );
    }
    // And something nothing denied still gets through.
    assert_eq!(
        verdict(&engine, PendingCall::tool("git.status"), &Subject::Agent),
        Verdict::Allow
    );
}

/// A sub-agent's allow list is narrowed by its parent's, never widened.
#[test]
fn child_is_intersected() {
    let ws = Workspace::new();
    let toml = r#"
[permissions]
allow = ["tool(git.*)"]

[permissions."agent:critic"]
allow = ["tool(git.*)", "tool(shell.exec)"]
"#;
    let engine = engine_of(&ws, &[(Layer::Project, toml)]);
    let critic = Subject::SubAgent("critic".to_owned());

    // What the parent also allows, the child gets.
    assert_eq!(
        verdict(&engine, PendingCall::tool("git.status"), &critic),
        Verdict::Allow
    );
    // What only the child's own file allows, it does not.
    assert_eq!(
        verdict(&engine, PendingCall::tool("shell.exec"), &critic),
        Verdict::Deny,
        "a rule file narrows a sub-agent; it never widens one"
    );
}

/// The same for an extension, which is the shape plan 06 leans on.
#[test]
fn an_extension_is_intersected_too() {
    let ws = Workspace::new();
    let toml = r#"
[permissions]
allow = ["read(./**)", "spawn(bazel *)"]

[permissions."ext:buildgraph"]
allow = ["spawn(bazel *)", "read(./**)"]
deny  = ["creds(*)"]
"#;
    let engine = engine_of(&ws, &[(Layer::Project, toml)]);
    let ext = Subject::Ext(ExtId::new("buildgraph").expect("a valid id"));

    assert_eq!(
        verdict(&engine, PendingCall::spawn("bazel build //..."), &ext),
        Verdict::Allow
    );
    assert_eq!(
        verdict(&engine, PendingCall::creds("NPM_TOKEN"), &ext),
        Verdict::Deny
    );
}

/// Translation #3: the TOML keys and plan 01's `Subject` are the same strings.
#[test]
fn subject_string_forms() {
    let ws = Workspace::new();
    let toml = r#"
[permissions]
allow = ["tool(a.*)", "tool(b.*)", "tool(c.*)"]

[permissions."ext:buildgraph"]
allow = ["tool(b.*)"]

[permissions."agent:critic"]
allow = ["tool(c.*)"]
"#;
    let engine = engine_of(&ws, &[(Layer::Project, toml)]);

    let cases: &[(&str, Subject, &str)] = &[
        ("agent", Subject::Agent, "a.read"),
        (
            "ext:buildgraph",
            Subject::Ext(ExtId::new("buildgraph").unwrap()),
            "b.read",
        ),
        (
            "agent:critic",
            Subject::SubAgent("critic".to_owned()),
            "c.read",
        ),
    ];
    for (key, subject, tool) in cases {
        // The key round-trips through `Subject`, in both directions.
        assert_eq!(&key.parse::<Subject>().expect("a subject"), subject);
        assert_eq!(&subject.to_string(), key);
        // And the rules under that key are the ones that apply to it.
        assert_eq!(
            verdict(&engine, PendingCall::tool(*tool), subject),
            Verdict::Allow,
            "{key} should reach {tool}"
        );
    }
}

/// `ArcSwap`, so a config reload does not block a single in-flight `check`.
#[test]
fn a_reload_swaps_without_rebuilding_the_engine() {
    let ws = Workspace::new();
    let engine = engine_of(&ws, &[(Layer::Project, "[permissions]\nallow = [\"tool(git.*)\"]\n")]);
    assert_eq!(
        verdict(&engine, PendingCall::tool("git.status"), &Subject::Agent),
        Verdict::Allow
    );

    engine.reload(
        PolicyBuilder::new(ws.root())
            .layer_toml(
                "[permissions]\ndeny = [\"tool(git.*)\"]\n",
                "reloaded.toml",
                Layer::Project,
                false,
            )
            .unwrap()
            .build()
            .unwrap(),
    );
    assert_eq!(
        verdict(&engine, PendingCall::tool("git.status"), &Subject::Agent),
        Verdict::Deny
    );
}

/// Every decision reaches the audit with the rule that produced it.
#[test]
fn every_decision_names_its_rule_in_the_audit() {
    let ws = Workspace::new();
    let audit = orrery_audit::memory();
    let engine = engine_of(&ws, &[(Layer::Managed, "[permissions]\ndeny = [\"creds(*)\"]\n")])
        .with_audit(Arc::clone(&audit) as orrery_audit::Audit);

    let _ = verdict(&engine, PendingCall::creds("ANTHROPIC_API_KEY"), &Subject::Agent);

    let record = audit
        .records()
        .into_iter()
        .find_map(|r| match r.event {
            orrery_audit::AuditEvent::CapabilityDecision {
                verdict,
                rule_text,
                layer,
                ..
            } => Some((verdict, rule_text, layer)),
            _ => None,
        })
        .expect("the decision is in the audit");
    assert_eq!(record.0, orrery_audit::Verdict::Deny);
    assert_eq!(record.1.as_deref(), Some("creds(*)"));
    assert_eq!(record.2, Some(Layer::Managed));

    // And the credential NAME is what appears — there is no value anywhere.
    assert!(!audit.to_jsonl().contains("sk-"));
}
