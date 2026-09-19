//! `explain` and `check` are one answer, or they are a permission system that
//! reports a verdict it does not enforce.
//!
//! This is the invariant, not an example of it: over rule sets, subjects,
//! calls and scopes, the verdict `PolicyEngine::explain` prints must be the
//! verdict `PolicyEngine::check` enforces. It was not — `check` inherited the
//! parent's rules for a subject nobody wrote about and `explain` did not, so a
//! sub-agent with no rules of its own was explained as denied everything while
//! a turn ran fine. The tool list the model is offered is built from `explain`,
//! which is how a default workspace came to offer zero tools.

mod common;

use common::{Workspace, wide_scope};
use orrery_policy::{PendingCall, PolicyBuilder, PolicyEngine, Rule, RuleList};
use orrery_proto::{AgentScope, Aspect, BranchId, Capability, ExtId, Grant, Layer, Subject};
use proptest::prelude::*;

/// The subjects a rule can be written about, and a call can be made by.
fn subjects() -> Vec<Subject> {
    vec![
        Subject::Agent,
        Subject::SubAgent("main".to_owned()),
        Subject::SubAgent("planner".to_owned()),
        Subject::Ext(ExtId::new("ripgrep").expect("a valid ext id")),
    ]
}

/// Aspect, rule pattern, and a call target that pattern is about.
const SHAPES: [(Aspect, &str, &str); 4] = [
    (Aspect::Tool, "tool(*)", "builtin.read"),
    (Aspect::Tool, "tool(git.*)", "git.status"),
    (Aspect::Read, "read(./**)", "./src/main.rs"),
    (Aspect::Net, "net(domain: *)", "example.com"),
];

fn lists() -> Vec<RuleList> {
    vec![RuleList::Deny, RuleList::Ask, RuleList::Allow]
}

prop_compose! {
    /// One rule: whom it is about, which list, which pattern.
    fn a_rule()(subject in 0usize..4, list in 0usize..3, shape in 0usize..4) -> Rule {
        let (aspect, text, _) = SHAPES[shape];
        Rule::new(lists()[list], Layer::Project, subjects()[subject].clone(), aspect, text)
    }
}

prop_compose! {
    /// A scope: sometimes an empty grant, which narrows nothing, sometimes one
    /// that carries a single aspect and therefore refuses every other.
    fn a_scope()(agent in 0usize..3, narrow in any::<bool>(), shape in 0usize..4) -> AgentScope {
        const NAMES: [&str; 3] = ["main", "planner", "test"];
        let grant = if narrow {
            Grant {
                capabilities: vec![Capability::scoped(SHAPES[shape].0, ["*".to_owned()])],
                ..Grant::nothing()
            }
        } else {
            Grant::nothing()
        };
        AgentScope {
            agent: NAMES[agent].to_owned(),
            branch: BranchId::new(),
            tools: vec!["*".to_owned()],
            grant,
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// For every rule set, subject, call and scope, the explanation and the
    /// decision are the same verdict.
    #[test]
    fn explain_agrees_with_check(
        rules in proptest::collection::vec(a_rule(), 0..6),
        subject in 0usize..4,
        shape in 0usize..4,
        scope in a_scope(),
    ) {
        let ws = Workspace::new();
        let engine = PolicyEngine::new(
            PolicyBuilder::new(ws.root())
                .layer_rules(Layer::Project, rules)
                .build()
                .expect("the fixture compiles"),
        );
        let subject = subjects()[subject].clone();
        let (aspect, _, target) = SHAPES[shape];
        let call = PendingCall::new(aspect, target);

        // With no scope in the question, the answer is the one a scope that
        // narrows nothing gets.
        prop_assert_eq!(
            engine.explain(&call, &subject).verdict,
            engine.check(&call, &subject, &wide_scope()).verdict(),
            "explain and check disagree for {} on {}",
            subject,
            call
        );

        // And with one, the scope's own ceiling is on both sides.
        prop_assert_eq!(
            engine.explain_in(&call, &subject, &scope).verdict,
            engine.check(&call, &subject, &scope).verdict(),
            "explain and check disagree for {} on {} under {}",
            subject,
            call,
            scope.agent
        );
    }
}

/// The instance that bit: a sub-agent nobody wrote a rule about is explained
/// with the rules it actually runs under, not denied everything.
#[test]
fn a_subject_with_no_rules_is_explained_by_what_it_inherits() {
    let ws = Workspace::new();
    let engine = PolicyEngine::new(
        PolicyBuilder::new(ws.root())
            .layer_toml(
                "[permissions]\nallow = [\"tool(*)\"]\n",
                "orrery.toml",
                Layer::Project,
                false,
            )
            .expect("the fixture parses")
            .build()
            .expect("the fixture compiles"),
    );

    let call = PendingCall::tool("builtin.read");
    let main = Subject::SubAgent("main".to_owned());
    let explained = engine.explain(&call, &main);

    assert_eq!(
        explained.verdict,
        engine.check(&call, &main, &wide_scope()).verdict(),
        "the explanation is the decision: {explained}"
    );
    assert_eq!(explained.verdict, orrery_policy::Verdict::Allow);
    assert!(
        explained.reason.contains("agent"),
        "the explanation says whose rule answered: {}",
        explained.reason
    );
}
