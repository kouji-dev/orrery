//! Task 4 · modes are permissions. The policy engine answers, not the router.

use orrery_policy::{PolicyBuilder, PolicyEngine, Verdict};
use orrery_proto::{AgentScope, Aspect, BranchId, Grant, Layer};
use orrery_router::Mode;
use orrery_router::mode::{self, ToolAspects};

fn scope() -> AgentScope {
    AgentScope {
        agent: "main".to_owned(),
        branch: BranchId::new(),
        tools: vec!["*".to_owned()],
        grant: Grant::nothing(),
    }
}

fn engine(root: &std::path::Path, toml: &str) -> PolicyEngine {
    let rules = PolicyBuilder::new(root)
        .layer_toml(toml, "config.toml", Layer::Project, false)
        .expect("the fixture parses")
        .build()
        .expect("the fixture compiles");
    PolicyEngine::new(rules)
}

#[test]
fn switch_is_a_policy_question() {
    let dir = tempfile::tempdir().expect("a temp dir");
    let root = dunce::canonicalize(dir.path()).expect("canonical root");
    let strict = engine(
        &root,
        "[permissions]\nallow = [\"mode(plan)\", \"mode(review)\"]\ndeny = [\"mode(execute)\"]\n",
    );
    let subject = orrery_proto::Subject::Agent;

    // The router builds the request and hands it over. It decides nothing.
    let asked = mode::switch(Mode::Execute);
    assert_eq!(asked.aspect, Aspect::Mode);
    assert_eq!(asked.target, "execute");
    assert_eq!(asked.match_text(), "mode(execute)");

    let refused = strict.check(&asked, &subject, &scope());
    assert_eq!(
        refused.verdict(),
        Verdict::Deny,
        "the policy engine refuses it — `this profile may never enter execute` is one line"
    );

    let allowed = strict.check(&mode::switch(Mode::Plan), &subject, &scope());
    assert_eq!(allowed.verdict(), Verdict::Allow);

    // And with no rule at all, the default is still the policy engine's: a
    // refusal, because a permission system whose default is yes is one in name.
    let silent = engine(&root, "[permissions]\nallow = [\"read(./**)\"]\n");
    assert_eq!(
        silent
            .check(&mode::switch(Mode::Execute), &subject, &scope())
            .verdict(),
        Verdict::Deny
    );
}

#[test]
fn plan_is_read_only() {
    let tools = [
        ToolAspects::new("fs.read", &[Aspect::Read]),
        ToolAspects::new("fs.write", &[Aspect::Write]),
        ToolAspects::new("shell.exec", &[Aspect::Spawn]),
        ToolAspects::new("mem.note", &[Aspect::MemWrite]),
        ToolAspects::new("net.fetch", &[Aspect::Net]),
    ];

    let visible = mode::visible(&tools, Mode::Plan);
    assert!(
        !visible.contains(&"fs.write"),
        "in plan mode a write tool is not in visible(): {visible:?}"
    );
    assert!(!visible.contains(&"shell.exec"));
    assert!(!visible.contains(&"mem.note"));
    assert!(visible.contains(&"fs.read"));

    // `review` never writes either.
    assert!(!mode::visible(&tools, Mode::Review).contains(&"fs.write"));

    // `execute` is the full granted set — the rules narrow it, the mode does not.
    let all = mode::visible(&tools, Mode::Execute);
    assert_eq!(all.len(), tools.len());
}

#[test]
fn every_mode_has_a_word_a_rule_can_name() {
    for mode in Mode::all() {
        assert_eq!(mode::switch(mode).target, mode.as_str());
    }
}
