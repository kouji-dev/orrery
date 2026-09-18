//! Task 1 · layer loading, the merge, and provenance.

mod common;

use common::Fixture;
use orrery_config::{ConfigPaths, LayerFile, merge};
use orrery_policy::{PendingCall, PolicyEngine};
use orrery_proto::{AgentScope, BranchId, Grant, Layer, Subject};

fn scope() -> AgentScope {
    AgentScope {
        agent: "test".to_owned(),
        branch: BranchId::new(),
        tools: vec!["*".to_owned()],
        grant: Grant::nothing(),
    }
}

#[test]
fn closest_layer_wins_for_values() {
    let fx = Fixture::new();
    fx.write("home/.orrery/config.toml", "model = \"user-model\"\ntheme = \"dark\"\n");
    fx.write_in_root(".orrery/config.toml", "model = \"project-model\"\n");

    let files = fx.paths().collect().expect("layers load");
    let report = merge::merge(&files).expect("the merge succeeds");

    let winner = report.values.winner("model").expect("model is set");
    assert_eq!(winner.value.as_str(), Some("project-model"));
    assert_eq!(winner.origin.layer, Layer::Workspace);
    assert!(
        winner.origin.file.ends_with("config.toml"),
        "provenance names the file: {}",
        winner.origin.file.display()
    );
    assert_eq!(winner.origin.line, 1, "provenance names the line");

    let shadowed = report.values.shadowed("model");
    assert_eq!(shadowed.len(), 1, "the user value is shadowed, not lost");
    assert_eq!(shadowed[0].origin.layer, Layer::User);

    // A key only the user set survives.
    assert_eq!(
        report.values.winner("theme").map(|s| s.value.as_str()),
        Some(Some("dark"))
    );
}

#[test]
fn deny_is_a_union() {
    let files = vec![
        LayerFile::new(Layer::Managed, "managed.toml", "[permissions]\ndeny = [\"creds(*)\"]\n"),
        LayerFile::new(Layer::User, "user.toml", "[permissions]\ndeny = [\"net(domain: *)\"]\n"),
        LayerFile::new(
            Layer::Project,
            "project.toml",
            "[permissions]\ndeny = [\"write(./secrets/**)\"]\n",
        ),
    ];
    let report = merge::merge(&files).expect("the merge succeeds");

    let denies: Vec<String> = report
        .values
        .union_strings("permissions.deny")
        .into_iter()
        .map(|(text, _)| text)
        .collect();
    assert_eq!(denies.len(), 3, "every layer's deny survives: {denies:?}");
    for want in ["creds(*)", "net(domain: *)", "write(./secrets/**)"] {
        assert!(denies.iter().any(|d| d == want), "{want} survives");
    }
    // And each one still knows where it came from.
    let layers: Vec<Layer> = report
        .values
        .union_strings("permissions.deny")
        .into_iter()
        .map(|(_, origin)| origin.layer)
        .collect();
    assert!(layers.contains(&Layer::Managed));
    assert!(layers.contains(&Layer::Project));
}

#[test]
fn managed_deny_cannot_be_relaxed() {
    let fx = Fixture::new();
    let files = vec![
        LayerFile::new(Layer::Managed, "managed.toml", "[permissions]\ndeny = [\"net(domain: *)\"]\n"),
        LayerFile::new(
            Layer::User,
            "user.toml",
            "[permissions]\nallow = [\"net(domain: *)\", \"read(./**)\"]\n",
        ),
    ];
    let report = merge::merge(&files).expect("the merge succeeds");

    // The attempt is logged, naming the rule and where it was written.
    assert_eq!(report.relaxations.len(), 1, "{:?}", report.relaxations);
    let attempt = &report.relaxations[0];
    assert_eq!(attempt.rule, "net(domain: *)");
    assert_eq!(attempt.attempted.layer, Layer::User);
    assert_eq!(attempt.managed.layer, Layer::Managed);

    // And the deny stands where it counts: in the engine the merge feeds.
    let rules = merge::policy(fx.root(), &files).expect("rules compile");
    let engine = PolicyEngine::new(rules);
    let decision = engine.check(&PendingCall::net("example.com"), &Subject::Agent, &scope());
    assert!(decision.verdict() == orrery_policy::Verdict::Deny, "{decision:?}");
}

#[test]
fn nested_project_closest_wins() {
    let fx = Fixture::new();
    fx.write_in_root(".orrery/config.toml", "model = \"workspace\"\n");
    fx.write_in_root("apps/.orrery/config.toml", "model = \"apps\"\n");
    fx.write_in_root("apps/web/.orrery/config.toml", "model = \"apps-web\"\n");
    std::fs::create_dir_all(fx.root().join("apps/web/src")).unwrap();

    let paths = ConfigPaths::sandboxed(fx.root(), fx.home().join(".orrery"))
        .with_cwd(fx.root().join("apps/web/src"));
    let files = paths.collect().expect("layers load");
    let report = merge::merge(&files).expect("the merge succeeds");

    let winner = report.values.winner("model").expect("model is set");
    assert_eq!(winner.value.as_str(), Some("apps-web"), "the nearer wins");
    assert_eq!(winner.origin.layer, Layer::Project);
    let shadowed: Vec<&str> = report
        .values
        .shadowed("model")
        .iter()
        .filter_map(|s| s.value.as_str())
        .collect();
    assert_eq!(shadowed, vec!["apps", "workspace"], "nearest first");
}
