//! Task 7 · the workflow machine: tool steps, joins, gates and the ceiling.

mod common;

use common::Fake;
use orrery_orchestrator::Catalogue;
use orrery_orchestrator::typecheck::Workflow;
use orrery_orchestrator::workflow::{Outcome, Runner};
use orrery_proto::Budget;

fn load(text: &str) -> Workflow {
    Workflow::load(text, "wf.toml", &Catalogue::empty()).expect("it loads")
}

fn budget(max_tokens: u64) -> Budget {
    Budget {
        max_turns: 100,
        max_tokens,
        wall_clock_ms: 600_000,
        max_micro_usd: None,
    }
}

#[tokio::test]
async fn tool_step_makes_no_model_call() {
    // Three deterministic steps, chained through `ref`. Deterministic steps
    // between model calls are where cost comes out, because they cost nothing.
    let text = r#"
name = "count"

[[step]]
name = "list"
kind = "tool"
ref = "fs.glob"
input = "crates/**"

[[step]]
name = "count"
kind = "tool"
ref = "fs.count"
input = { ref = "list", path = ["files"] }

[[step]]
name = "report"
kind = "tool"
ref = "fs.write"
input = { ref = "count" }
"#;
    let exec = Fake::new()
        .answering("fs.glob", serde_json::json!({ "files": ["a.rs", "b.rs"] }))
        .answering("fs.count", serde_json::json!({ "count": 2 }))
        .costing(9_999);

    let run = Runner::new(&exec)
        .run(&load(text), Some(budget(1_000_000)))
        .await;

    assert_eq!(run.outcome, Outcome::Completed);
    assert_eq!(exec.model_calls(), 0, "the provider was never invoked");
    assert_eq!(exec.tool_calls().len(), 3);
    assert_eq!(
        run.usage.total_tokens(),
        0,
        "and the whole run cost nothing at all"
    );
    // The `ref` actually carried the earlier step's value.
    assert_eq!(
        exec.tool_calls()[1].input,
        serde_json::json!(["a.rs", "b.rs"])
    );
}

#[tokio::test]
async fn parallel_join_all_first_quorum() {
    // Three branches: `quick` answers at once, `slower` and `slowest` take
    // their time. Three cases over the same shape.
    let text = |join: &str| {
        format!(
            r#"
name = "wide"

[[step]]
name = "fan"
kind = "parallel"
join = {join}

[[step.steps]]
name = "quick"
kind = "agent"
subagent = "quick"
input = "a"

[[step.steps]]
name = "slower"
kind = "agent"
subagent = "slower"
input = "b"

[[step.steps]]
name = "slowest"
kind = "agent"
subagent = "slowest"
input = "c"
"#
        )
    };
    let fake = || {
        Fake::new()
            .slow("slower", 40)
            .slow("slowest", 200)
            .costing(10)
    };

    // 1 · all — every branch answers, in declaration order.
    let exec = fake();
    let run = Runner::new(&exec)
        .run(&load(&text("\"all\"")), Some(budget(1_000_000)))
        .await;
    assert_eq!(run.outcome, Outcome::Completed);
    assert_eq!(exec.model_calls(), 3);
    for name in ["quick", "slower", "slowest"] {
        assert!(run.env.has(name), "`{name}` answered");
    }
    assert_eq!(
        run.env.get("fan").and_then(|v| v.get("cancelled")),
        Some(&serde_json::json!(0)),
        "nothing to cancel under `all`"
    );

    // 2 · first — one answer, the other two cancelled.
    let exec = fake();
    let run = Runner::new(&exec)
        .run(&load(&text("\"first\"")), Some(budget(1_000_000)))
        .await;
    assert_eq!(run.outcome, Outcome::Completed);
    assert!(run.env.has("quick"), "the quick one won");
    assert!(!run.env.has("slowest"), "and the slow one was cancelled");
    assert_eq!(
        run.env.get("fan").and_then(|v| v.get("cancelled")),
        Some(&serde_json::json!(2))
    );

    // 3 · quorum — two answers, the last cancelled rather than waited for
    //     (open question 3, decided: cancelling is cheaper and is intended).
    let exec = fake();
    let run = Runner::new(&exec)
        .run(&load(&text("{ quorum = 2 }")), Some(budget(1_000_000)))
        .await;
    assert_eq!(run.outcome, Outcome::Completed);
    assert!(run.env.has("quick") && run.env.has("slower"));
    assert!(!run.env.has("slowest"), "the third was cancelled, not used");
    assert_eq!(
        run.env.get("fan").and_then(|v| v.get("cancelled")),
        Some(&serde_json::json!(1))
    );
}

#[tokio::test]
async fn gate_on_fail() {
    // A gate over a tool result that is always 3.
    let text = |on_fail: &str| {
        format!(
            r#"
name = "gated"

[[step]]
name = "tests"
kind = "tool"
ref = "cargo.test"
input = "-p x"

[[step]]
name = "gate"
kind = "gate"
on_fail = "{on_fail}"
check = {{ cmp = {{ lhs = {{ ref = "tests", path = ["failures"] }}, op = "eq", rhs = 0 }} }}

[[step]]
name = "after"
kind = "agent"
subagent = "executor"
input = "ship it"
"#
        )
    };
    let failing = || Fake::new().answering("cargo.test", serde_json::json!({ "failures": 3 }));

    // stop — the workflow ends at the gate, and `after` never runs.
    let exec = failing();
    let run = Runner::new(&exec)
        .run(&load(&text("stop")), Some(budget(1_000_000)))
        .await;
    assert_eq!(
        run.outcome,
        Outcome::StoppedByGate {
            step: "gate".to_owned()
        }
    );
    assert_eq!(exec.model_calls(), 0, "nothing after the gate ran");

    // retry — the step before the gate runs once more, then the gate is asked
    // again and the answer is the same.
    let exec = failing();
    let run = Runner::new(&exec)
        .run(&load(&text("retry")), Some(budget(1_000_000)))
        .await;
    assert_eq!(
        run.outcome,
        Outcome::StoppedByGate {
            step: "gate".to_owned()
        }
    );
    assert_eq!(exec.tool_calls().len(), 2, "the step before it ran twice");
    assert_eq!(exec.model_calls(), 0);

    // escalate — the decision goes up to the router rather than ending here.
    let exec = failing();
    let run = Runner::new(&exec)
        .run(&load(&text("escalate")), Some(budget(1_000_000)))
        .await;
    assert_eq!(
        run.outcome,
        Outcome::Escalated {
            step: "gate".to_owned()
        }
    );

    // And a gate that passes is invisible.
    let exec = Fake::new().answering("cargo.test", serde_json::json!({ "failures": 0 }));
    let run = Runner::new(&exec)
        .run(&load(&text("stop")), Some(budget(1_000_000)))
        .await;
    assert_eq!(run.outcome, Outcome::Completed);
    assert_eq!(exec.model_calls(), 1, "`after` ran");
}

#[tokio::test]
async fn budget_is_whole_workflow() {
    // Four agent steps at 100 tokens each, under a 250-token ceiling. A
    // per-step budget would have let all four through; the ceiling applies
    // across steps, so it stops after the third has taken it over.
    let text = r#"
name = "long"

[[step]]
name = "a"
kind = "agent"
subagent = "executor"
input = "1"

[[step]]
name = "b"
kind = "agent"
subagent = "executor"
input = "2"

[[step]]
name = "c"
kind = "agent"
subagent = "executor"
input = "3"

[[step]]
name = "d"
kind = "agent"
subagent = "executor"
input = "4"
"#;
    let exec = Fake::new().costing(100);
    let run = Runner::new(&exec).run(&load(text), Some(budget(250))).await;

    assert!(
        matches!(run.outcome, Outcome::StoppedByBudget { .. }),
        "{:?}",
        run.outcome
    );
    assert_eq!(exec.model_calls(), 3, "and it did not run the fourth");
    assert_eq!(run.usage.total_tokens(), 300);
    assert!(run.env.has("c") && !run.env.has("d"));

    // Each step is handed a slice of what is **left**, never the whole ceiling.
    let budgets: Vec<u64> = exec
        .agent_calls()
        .iter()
        .filter_map(|c| c.budget.map(|b| b.max_tokens))
        .collect();
    assert_eq!(budgets, vec![250, 150, 50]);
}

#[tokio::test]
async fn a_failing_step_names_itself() {
    let text = r#"
name = "broken"

[[step]]
name = "nope"
kind = "agent"
subagent = "executor"
input = "go"
"#;
    let exec = Fake::new().failing("executor");
    let run = Runner::new(&exec)
        .run(&load(text), Some(budget(1_000)))
        .await;
    match run.outcome {
        Outcome::Failed { step, code, .. } => {
            assert_eq!(step, "nope");
            assert_eq!(code, "agent-failed");
        }
        other => panic!("{other:?}"),
    }
}
