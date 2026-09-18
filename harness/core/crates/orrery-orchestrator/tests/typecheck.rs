//! Task 5 · translation #5: the whole workflow dataflow is typechecked AT LOAD.
//!
//! A workflow must not fail mid-run after paying for three model calls.

use orrery_orchestrator::step::{AgentDefinition, Catalogue, ToolDefinition, TypeShape};
use orrery_orchestrator::typecheck::{Checked, LoadError, Workflow, check};
use orrery_proto::Budget;

fn budget() -> Budget {
    Budget {
        max_turns: 4,
        max_tokens: 50_000,
        wall_clock_ms: 120_000,
        max_micro_usd: None,
    }
}

/// A catalogue where `plan` returns `{ files: [text] }` and `count` a number.
fn catalogue() -> Catalogue {
    Catalogue::empty()
        .with_agent(
            AgentDefinition::new("planner", budget()).returning(TypeShape::object([(
                "files",
                TypeShape::Array(Box::new(TypeShape::Text)),
            )])),
        )
        .with_agent(AgentDefinition::new("executor", budget()))
        .with_tool(ToolDefinition::new(
            "fs.count",
            TypeShape::object([("count", TypeShape::Number)]),
        ))
}

#[test]
fn forward_ref_is_rejected_at_load() {
    // Step 1 reads step 3. Three model calls would have been paid for before
    // anything noticed, so nothing runs at all.
    let text = r#"
name = "release"

[[step]]
name = "one"
kind = "agent"
subagent = "planner"
input = { ref = "three" }

[[step]]
name = "two"
kind = "agent"
subagent = "executor"
input = "go"

[[step]]
name = "three"
kind = "agent"
subagent = "executor"
input = "go"
"#;
    let err = Workflow::load(text, "release.toml", &catalogue()).expect_err("it fails at load");
    match &err {
        LoadError::ForwardRef { file, step, target } => {
            assert_eq!(file, "release.toml", "it names the file");
            assert_eq!(step, "one", "and the step");
            assert_eq!(target, "three");
        }
        other => panic!("a forward reference: {other:?}"),
    }
    assert!(err.to_string().contains("runs later"), "{err}");

    // A step that is not declared anywhere is a different mistake, with a
    // different message.
    let missing = text.replace("\"three\" }", "\"nowhere\" }");
    let err = Workflow::load(&missing, "release.toml", &catalogue()).expect_err("also fails");
    assert!(matches!(err, LoadError::NoSuchStep { .. }), "{err:?}");
}

#[test]
fn path_must_match_returns() {
    // `.count` on a step whose `returns` has no `count`.
    let text = r#"
name = "release"

[[step]]
name = "plan"
kind = "agent"
subagent = "planner"
input = "go"

[[step]]
name = "use"
kind = "agent"
subagent = "executor"
input = { ref = "plan", path = ["count"] }
"#;
    let err = Workflow::load(text, "release.toml", &catalogue()).expect_err("it fails at load");
    match &err {
        LoadError::BadPath(bad) => {
            assert_eq!(bad.file, "release.toml");
            assert_eq!(bad.step, "use");
            assert_eq!(bad.target, "plan");
            assert_eq!(bad.path, vec!["count".to_owned()]);
            assert_eq!(bad.keys, vec!["files".to_owned()], "it says what is there");
        }
        other => panic!("a bad path: {other:?}"),
    }

    // The path that *is* declared loads.
    let good = text.replace("[\"count\"]", "[\"files\"]");
    Workflow::load(&good, "release.toml", &catalogue()).expect("`files` is declared");

    // An undeclared return is unchecked rather than wrong: `executor` declares
    // nothing, so a path into it is accepted and the run finds out.
    let unchecked = r#"
name = "release"

[[step]]
name = "run"
kind = "agent"
subagent = "executor"
input = "go"

[[step]]
name = "after"
kind = "agent"
subagent = "executor"
input = { ref = "run", path = ["whatever"] }
"#;
    Workflow::load(unchecked, "release.toml", &catalogue()).expect("Any is unchecked");
}

#[test]
fn valid_workflow_passes() {
    let text = r#"
name = "release"
budget = { max_turns = 8, max_tokens = 200000, wall_clock_ms = 600000 }

[[step]]
name = "plan"
kind = "agent"
subagent = "planner"
input = "work out what to change"

[[step]]
name = "count"
kind = "tool"
ref = "fs.count"
input = { ref = "plan", path = ["files"] }

[[step]]
name = "gate"
kind = "gate"
on_fail = "stop"
check = { cmp = { lhs = { ref = "count", path = ["count"] }, op = "gt", rhs = 0 } }

[[step]]
name = "do"
kind = "agent"
subagent = "executor"
input = { ref = "plan" }
"#;
    let workflow = Workflow::load(text, "release.toml", &catalogue()).expect("it loads");
    assert_eq!(workflow.name, "release");
    assert_eq!(workflow.steps.len(), 4);
    assert_eq!(workflow.budget.max_turns, 8);
    // Checking twice changes nothing.
    check(&workflow, &catalogue()).expect("idempotent");
}

#[test]
fn a_loop_without_until_does_not_load() {
    // Task 7 · `loops::predicate_is_mandatory`, asserted where loading happens.
    let text = r#"
name = "verify"

[[step]]
name = "again"
kind = "loop"
max_iterations = 3
body = []
"#;
    let err = Workflow::from_toml_str(text, "verify.toml").expect_err("it fails at load");
    assert!(matches!(err, LoadError::Syntax { .. }), "{err:?}");
    assert!(err.to_string().contains("until"), "{err}");

    // And with the predicate written down it loads.
    let with_until = format!("{text}until = {{ all = [] }}\n");
    Workflow::from_toml_str(&with_until, "verify.toml").expect("it loads");
}

#[test]
fn a_zero_cap_is_a_load_error() {
    let text = r#"
name = "verify"

[[step]]
name = "again"
kind = "loop"
max_iterations = 0
until = { all = [] }
body = []
"#;
    let err = Workflow::load(text, "verify.toml", &Catalogue::empty()).expect_err("fails");
    assert!(matches!(err, LoadError::ZeroCap { .. }), "{err:?}");
}

#[test]
fn a_duplicate_step_name_is_ambiguous() {
    let text = r#"
name = "release"

[[step]]
name = "same"
kind = "tool"
ref = "fs.count"
input = 1

[[step]]
name = "same"
kind = "tool"
ref = "fs.count"
input = 2
"#;
    let err = Workflow::load(text, "release.toml", &catalogue()).expect_err("fails");
    assert!(matches!(err, LoadError::DuplicateStep { .. }), "{err:?}");
}

#[test]
fn a_parallel_branch_may_not_read_its_sibling() {
    // They run at the same time, so a `ref` across them is a forward reference
    // whatever the declaration order says.
    let text = r#"
name = "wide"

[[step]]
name = "both"
kind = "parallel"
join = "all"

[[step.steps]]
name = "left"
kind = "agent"
subagent = "executor"
input = "a"

[[step.steps]]
name = "right"
kind = "agent"
subagent = "executor"
input = { ref = "left" }
"#;
    let err = Workflow::load(text, "wide.toml", &catalogue()).expect_err("fails at load");
    assert!(matches!(err, LoadError::ForwardRef { .. }), "{err:?}");
}

#[test]
fn only_the_typechecker_can_make_a_checked_workflow() {
    // Translation #5 says an invalid workflow fails at load. That is a claim
    // about the *type*, not about API discipline: `Runner::run` takes a
    // `Checked`, and the only way to hold one is to have run `check`. The
    // negative half — handing the runner a plain `Workflow` — is a
    // `compile_fail` doctest on `Checked`, because a test that does not compile
    // cannot live in a test file.
    let bad = r#"
name = "release"

[[step]]
name = "one"
kind = "agent"
subagent = "planner"
input = { ref = "three" }

[[step]]
name = "three"
kind = "agent"
subagent = "executor"
input = "go"
"#;
    // The unchecked parse is still available, and still says nothing.
    let unchecked = Workflow::from_toml_str(bad, "release.toml").expect("it parses");
    assert_eq!(unchecked.steps.len(), 2);

    // Promoting it is the check, and it fails.
    let err = Checked::new(unchecked, &catalogue()).expect_err("it does not typecheck");
    assert!(matches!(err, LoadError::ForwardRef { .. }), "{err:?}");

    let good = r#"
name = "release"

[[step]]
name = "plan"
kind = "agent"
subagent = "planner"
input = "go"

[[step]]
name = "do"
kind = "agent"
subagent = "executor"
input = { ref = "plan", path = ["files"] }
"#;
    let checked: Checked = Workflow::load(good, "release.toml", &catalogue()).expect("it loads");
    // It reads as the workflow it wraps, and gives it back unchanged.
    assert_eq!(checked.name, "release");
    assert_eq!(checked.as_workflow().steps.len(), 2);
    assert_eq!(checked.into_inner().name, "release");
}
