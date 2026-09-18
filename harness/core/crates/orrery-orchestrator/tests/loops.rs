//! Task 7 · **the phase-6 acceptance criterion.**
//!
//! A verify loop whose gate never passes stops at `max_iterations`, and the
//! harness enforced it rather than the prompt.

mod common;

use common::Fake;
use orrery_orchestrator::typecheck::{Checked, Workflow};
use orrery_orchestrator::workflow::{Outcome, Runner};
use orrery_orchestrator::{Catalogue, LoadError};

/// A verify loop: fix, then test, then a gate that reads the test result.
///
/// The gate compares `tests.failures == 0`, and the executor's `test` agent
/// answers `{"failures": 3}` every single time. Nothing in the workflow, and
/// nothing given to the model, says "give up after four": the loop ends because
/// `max_iterations` is a counter the runner owns.
const VERIFY: &str = r#"
name = "verify"
budget = { max_turns = 100, max_tokens = 10000000, wall_clock_ms = 600000 }

[[step]]
name = "verify"
kind = "loop"
max_iterations = 4
until = { cmp = { lhs = { ref = "tests", path = ["failures"] }, op = "eq", rhs = 0 } }

[[step.body]]
name = "fix"
kind = "agent"
subagent = "executor"
input = "make the tests pass"

[[step.body]]
name = "tests"
kind = "tool"
ref = "cargo.test"
input = "-p orrery-orchestrator"

[[step.body]]
name = "gate"
kind = "gate"
on_fail = "stop"
check = { cmp = { lhs = { ref = "tests", path = ["failures"] }, op = "eq", rhs = 0 } }
"#;

#[tokio::test]
async fn terminates_on_its_own_cap() {
    // The gate never passes: three failures, every time round, for ever.
    let exec = Fake::new()
        .answering("cargo.test", serde_json::json!({ "failures": 3 }))
        .costing(100);
    let workflow = Workflow::load(VERIFY, "verify.toml", &Catalogue::empty()).expect("it loads");

    let run = Runner::new(&exec).run(&workflow, None).await;

    // It ended, and it ended *well*: a capped loop is an ordinary outcome, not
    // a failure and not a hang.
    assert_eq!(run.outcome, Outcome::Completed, "{:?}", run.outcome);

    // The typed outcome: this loop stopped on its cap, and how many times round
    // it went.
    let capped = run.cap("verify").expect("the cap is recorded, by name");
    assert_eq!(capped.iterations, 4, "exactly max_iterations, no more");
    assert_eq!(
        run.env.get("verify"),
        Some(&serde_json::json!({ "iterations": 4, "stopped_by": "cap" })),
        "and the step's own value says which of the two ended it"
    );

    // The harness enforced it, not the prompt: the body ran exactly four times,
    // the model was called exactly four times, and it was handed the same input
    // every single time — no instruction to stop was ever added to it.
    assert_eq!(exec.model_calls(), 4, "four passes, then the counter bit");
    assert_eq!(exec.tool_calls().len(), 4);
    let inputs: Vec<_> = exec.agent_calls().iter().map(|c| c.input.clone()).collect();
    assert_eq!(
        inputs,
        vec![serde_json::json!("make the tests pass"); 4],
        "the model was never told to stop; the counter stopped it"
    );

    // A gate inside the body ends the round, not the run: that is what a verify
    // loop is for.
    assert_eq!(
        run.env.get("gate"),
        Some(&serde_json::json!({ "passed": false }))
    );
}

#[tokio::test]
async fn a_passing_gate_ends_the_loop_early() {
    // The same workflow, with a gate that passes first time round.
    let exec = Fake::new()
        .answering("cargo.test", serde_json::json!({ "failures": 0 }))
        .costing(100);
    let workflow = Workflow::load(VERIFY, "verify.toml", &Catalogue::empty()).expect("it loads");

    let run = Runner::new(&exec).run(&workflow, None).await;

    assert_eq!(run.outcome, Outcome::Completed);
    assert!(run.cap("verify").is_none(), "no cap was reached");
    assert_eq!(
        run.env.get("verify"),
        Some(&serde_json::json!({ "iterations": 1, "stopped_by": "predicate" }))
    );
    assert_eq!(exec.model_calls(), 1, "and it did not go round again");
}

#[tokio::test]
async fn predicate_is_mandatory() {
    // A `Loop` step without `until` fails **at load**. The field has no serde
    // default, so there is no way to build one that would run.
    let no_until = r#"
name = "verify"

[[step]]
name = "again"
kind = "loop"
max_iterations = 4
body = []
"#;
    let err = Workflow::from_toml_str(no_until, "verify.toml").expect_err("it fails at load");
    assert!(matches!(err, LoadError::Syntax { .. }), "{err:?}");
    assert!(err.to_string().contains("until"), "{err}");
    assert_eq!(err.file(), "verify.toml");

    // And a cap is mandatory too — the other half of "nothing is a free-running
    // while".
    let no_cap = r#"
name = "verify"

[[step]]
name = "again"
kind = "loop"
until = { all = [] }
body = []
"#;
    let err = Workflow::from_toml_str(no_cap, "verify.toml").expect_err("it fails at load");
    assert!(err.to_string().contains("max_iterations"), "{err}");
}

#[tokio::test]
async fn the_loop_body_charges_the_whole_workflow_ceiling() {
    // 250 tokens a pass against a 600-token workflow: the ceiling bites before
    // the cap does, which is what "the ceiling applies across steps" means.
    let exec = Fake::new()
        .answering("cargo.test", serde_json::json!({ "failures": 3 }))
        .costing(250);
    let mut unchecked = Workflow::from_toml_str(VERIFY, "verify.toml").expect("it parses");
    unchecked.budget = orrery_proto::Budget {
        max_turns: 100,
        max_tokens: 600,
        wall_clock_ms: 600_000,
        max_micro_usd: None,
    };
    // Editing a workflow means typechecking it again before the runner sees it.
    let workflow = Checked::new(unchecked, &Catalogue::empty()).expect("it typechecks");

    let run = Runner::new(&exec).run(&workflow, None).await;
    assert!(
        matches!(run.outcome, Outcome::StoppedByBudget { .. }),
        "{:?}",
        run.outcome
    );
    assert!(exec.model_calls() < 4, "it stopped before the cap");
}
