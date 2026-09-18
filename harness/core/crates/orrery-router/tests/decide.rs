//! Task 2 · `decide`: precedence, the ladder, purity and the audit.

use std::sync::Arc;

use orrery_audit::{AuditEvent, MemorySink};
use orrery_proto::{AgentScope, BranchId, Budget, Grant, Usage};
use orrery_router::rules::RuleSet;
use orrery_router::{FanOutProfile, Proposal, RouteDecision, Router, RouterProfile, Rung, Signals};

fn scope() -> AgentScope {
    AgentScope {
        agent: "main".to_owned(),
        branch: BranchId::new(),
        tools: vec!["fs.read".to_owned()],
        grant: Grant::nothing(),
    }
}

fn signals() -> Signals {
    Signals {
        budget_spent: Usage {
            input_tokens: 10_000,
            output_tokens: 2_000,
            ..Usage::default()
        },
        budget_limit: Budget {
            max_turns: 20,
            max_tokens: 100_000,
            wall_clock_ms: 600_000,
            max_micro_usd: Some(2_000_000),
        },
        turns_in_mode: 3,
        diff_lines: 40,
        writes: 2,
        reads: 9,
        ..Signals::default()
    }
}

/// A rule forbidding fan-out outright.
fn no_fanout() -> RuleSet {
    RuleSet::parse_toml(
        "[[route]]\nname = \"never fan out\"\nwhen = { diff_lines = \"< 1000\" }\ndeny = \"fan-out\"\nreason = \"one problem, one agent\"\n",
        "config.toml",
    )
    .expect("the fixture parses")
}

fn router(rules: RuleSet, fanout: FanOutProfile) -> Router {
    Router::new(RouterProfile { rules, fanout })
}

#[test]
fn user_instruction_beats_a_rule() {
    let router = router(no_fanout(), FanOutProfile::capped(4));
    let inputs = vec![
        serde_json::json!(["crates/a"]),
        serde_json::json!(["crates/b"]),
    ];

    // The model asking for it is refused by the rule.
    let refused = router.decide(
        &signals(),
        &scope(),
        Some(
            Proposal::model(Rung::FanOut)
                .from(Rung::SubAgent)
                .with_agent("worker")
                .with_inputs(inputs.clone()),
        ),
    );
    assert!(
        matches!(&refused, RouteDecision::Deny { reason } if reason.contains("one problem")),
        "{refused:?}"
    );

    // The same request, instructed by a person, is granted.
    let granted = router.decide(
        &signals(),
        &scope(),
        Some(
            Proposal::user(Rung::FanOut)
                .from(Rung::SubAgent)
                .with_agent("worker")
                .with_inputs(inputs),
        ),
    );
    match granted {
        RouteDecision::FanOut {
            agent,
            inputs,
            budget_each,
        } => {
            assert_eq!(agent, "worker");
            assert_eq!(inputs.len(), 2);
            assert!(
                budget_each.max_tokens <= 88_000 / 2,
                "a child's slice never exceeds what is left"
            );
        }
        other => panic!("an explicit instruction wins: {other:?}"),
    }
}

#[test]
fn rule_beats_a_model_proposal() {
    let router = router(no_fanout(), FanOutProfile::capped(8));
    let decision = router.decide(
        &signals(),
        &scope(),
        Some(
            Proposal::model(Rung::FanOut)
                .from(Rung::SubAgent)
                .with_agent("worker")
                .with_inputs(vec![
                    serde_json::json!(["a"]),
                    serde_json::json!(["b"]),
                    serde_json::json!(["c"]),
                ]),
        ),
    );
    assert!(
        matches!(decision, RouteDecision::Deny { .. }),
        "{decision:?}"
    );
}

#[test]
fn rungs_are_climbed_one_at_a_time() {
    let router = router(RuleSet::empty(), FanOutProfile::capped(4));

    // One pass → workflow is three rungs at once. Downgraded to the next one up.
    let downgraded = router.decide(
        &signals(),
        &scope(),
        Some(
            Proposal::model(Rung::Workflow)
                .from(Rung::OnePass)
                .with_workflow("release"),
        ),
    );
    assert!(
        matches!(downgraded, RouteDecision::BoundedLoop { .. }),
        "a jump is downgraded to the next rung: {downgraded:?}"
    );

    // One rung at a time is granted as asked.
    let granted = router.decide(
        &signals(),
        &scope(),
        Some(
            Proposal::model(Rung::SubAgent)
                .from(Rung::BoundedLoop)
                .with_agent("worker"),
        ),
    );
    assert!(
        matches!(granted, RouteDecision::SubAgent { .. }),
        "{granted:?}"
    );

    // Unless explicitly instructed, which is the documented exception.
    let instructed = router.decide(
        &signals(),
        &scope(),
        Some(
            Proposal::user(Rung::Workflow)
                .from(Rung::OnePass)
                .with_workflow("release"),
        ),
    );
    assert!(
        matches!(&instructed, RouteDecision::Workflow { name } if name == "release"),
        "{instructed:?}"
    );
}

#[test]
fn a_loop_is_always_capped() {
    let router = router(RuleSet::empty(), FanOutProfile::default());
    let decision = router.decide(
        &signals(),
        &scope(),
        // No predicate, no cap: the model forgot to say when to stop.
        Some(Proposal::model(Rung::BoundedLoop)),
    );
    match decision {
        RouteDecision::BoundedLoop { max_iterations, .. } => {
            assert!(max_iterations >= 1, "nothing is a free-running while");
            assert_eq!(max_iterations, orrery_router::DEFAULT_MAX_ITERATIONS);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn is_pure() {
    let router = router(no_fanout(), FanOutProfile::priced(4, 10_000));
    let signals = signals();
    let scope = scope();
    let proposal = Proposal::model(Rung::SubAgent)
        .from(Rung::BoundedLoop)
        .with_agent("worker");

    let first = router.decide(&signals, &scope, Some(proposal.clone()));
    for i in 0..1000 {
        let again = router.decide(&signals, &scope, Some(proposal.clone()));
        assert_eq!(
            first, again,
            "same signals in, same decision out (round {i})"
        );
    }
}

#[test]
fn is_audited_with_signal_values() {
    let audit = Arc::new(MemorySink::new());
    let router = router(RuleSet::empty(), FanOutProfile::capped(4))
        .with_audit(Arc::clone(&audit) as orrery_audit::Audit);

    let signals = signals();
    let _ = router.decide(
        &signals,
        &scope(),
        Some(
            Proposal::model(Rung::SubAgent)
                .from(Rung::BoundedLoop)
                .with_agent("worker"),
        ),
    );

    let records = audit.records();
    assert_eq!(records.len(), 1, "one decision, one line");
    let AuditEvent::RoutingDecision {
        chose,
        signals: recorded,
    } = &records[0].event
    else {
        panic!("a routing decision: {:?}", records[0].event);
    };
    assert_eq!(chose, "sub-agent");
    // The numbers that produced it, not a summary of them.
    assert_eq!(recorded.get("diff_lines"), Some(&40.0));
    assert_eq!(recorded.get("writes"), Some(&2.0));
    assert_eq!(recorded.get("reads"), Some(&9.0));
    assert_eq!(recorded.get("turns_in_mode"), Some(&3.0));
    assert_eq!(recorded.get("tokens_spent"), Some(&12_000.0));
    assert_eq!(recorded.get("tokens_remaining"), Some(&88_000.0));
    assert_eq!(
        recorded.get("budget_fraction"),
        Some(&0.12),
        "so \"why five and not two\" is answerable afterwards"
    );
}

#[test]
fn an_explained_decision_names_the_rule() {
    let router = router(no_fanout(), FanOutProfile::capped(4));
    let explained = router.decide_explained(
        &signals(),
        &scope(),
        Some(
            Proposal::model(Rung::FanOut)
                .from(Rung::SubAgent)
                .with_agent("worker")
                .with_inputs(vec![serde_json::json!(["a"]), serde_json::json!(["b"])]),
        ),
    );
    assert_eq!(explained.rule.as_deref(), Some("never fan out"));
    assert_eq!(explained.asked, Some(Rung::FanOut));
    assert!(!explained.signals.is_empty());
}

#[test]
fn no_proposal_is_one_more_pass() {
    let router = router(RuleSet::empty(), FanOutProfile::default());
    assert_eq!(
        router.decide(&signals(), &scope(), None),
        RouteDecision::NextPass
    );
}
