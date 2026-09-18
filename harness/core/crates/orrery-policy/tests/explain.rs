//! Task 10 · `permissions explain`. The CLI surface is plan 17.

mod common;

use common::Workspace;
use orrery_policy::{PendingCall, PolicyBuilder, PolicyEngine, RuleList, Verdict};
use orrery_proto::{Layer, Subject};

#[test]
fn names_rule_layer_and_file() {
    let ws = Workspace::new();
    let managed = "\
[permissions]
deny = [
  \"creds(*)\",
  \"net(domain: *)\",
]
";
    let project = "[permissions]\nallow = [\"tool(git.*)\"]\n";
    let engine = PolicyEngine::new(
        PolicyBuilder::new(ws.root())
            .layer_toml(managed, "managed.toml", Layer::Managed, false)
            .unwrap()
            .layer_toml(project, "project.toml", Layer::Project, false)
            .unwrap()
            .build()
            .unwrap(),
    );

    let explained = engine.explain(&PendingCall::net("example.com"), &Subject::Agent);
    let rule = explained.rule.clone().expect("a rule decided");

    assert_eq!(explained.verdict, Verdict::Deny);
    assert_eq!(rule.text, "net(domain: *)");
    assert_eq!(rule.list, RuleList::Deny);
    assert_eq!(rule.layer, Layer::Managed);
    assert_eq!(rule.file.file_name().unwrap(), "managed.toml");
    assert_eq!(rule.line, 4, "the line the rule is written on");

    // And the rendered form carries all four, so plan 17 has nothing to invent.
    let rendered = explained.to_string();
    for needle in ["net(example.com)", "managed.toml", "Managed", "deny"] {
        assert!(rendered.contains(needle), "missing `{needle}` in:\n{rendered}");
    }
}

#[test]
fn explains_a_refusal_with_no_rule_behind_it() {
    let ws = Workspace::new();
    let engine = PolicyEngine::new(
        PolicyBuilder::new(ws.root())
            .layer_toml(
                "[permissions]\nallow = [\"tool(git.*)\"]\n",
                "project.toml",
                Layer::Project,
                false,
            )
            .unwrap()
            .build()
            .unwrap(),
    );
    let explained = engine.explain(&PendingCall::tool("shell.exec"), &Subject::Agent);
    assert_eq!(explained.verdict, Verdict::Deny);
    assert!(explained.rule.is_none());
    assert!(explained.reason.contains("no rule"), "{}", explained.reason);
    // The rules that were looked at and did not match are still reported, which
    // is what makes `explain` usable when the answer is "nothing matched".
    assert!(explained.considered.iter().any(|r| r.text == "tool(git.*)"));
}

/// A dry run decides nothing: it must not mint, and it must not spend.
#[test]
fn explain_is_a_dry_run() {
    let ws = Workspace::new();
    let engine = PolicyEngine::new(
        PolicyBuilder::new(ws.root())
            .layer_toml(
                "[permissions]\nallow = [\"tool(git.*)\"]\n",
                "project.toml",
                Layer::Project,
                false,
            )
            .unwrap()
            .build()
            .unwrap(),
    );
    let before = engine.ledger().live();
    let _ = engine.explain(&PendingCall::tool("git.status"), &Subject::Agent);
    assert_eq!(engine.ledger().live(), before, "explain minted a token");
}
