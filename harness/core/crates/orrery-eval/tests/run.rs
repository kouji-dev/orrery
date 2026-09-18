//! Task 2 and task 4: the run types, and where the numbers come from.
//!
//! `cost_comes_from_telemetry` is the test the whole plan exists for. Every
//! number here comes out of a committed fixture stream — no provider is
//! reachable, no key exists, and the expected totals are arithmetic over the
//! `.jsonl` files in `tests/fixtures/`.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use common::store::MemoryStore;
use common::{ScriptedProvider, fixture};
use orrery_eval::case::{EvalCase, GraderSpec, Suite};
use orrery_eval::matrix::Matrix;
use orrery_eval::report::CostProvenance;
use orrery_eval::run::{EvalRun, EvalRunner, HarnessRunner, MemoryMode, RoleBinding, RouterMode};
use orrery_eval::{EvalOutcome, GradeInput, Grader, Isolator, Score, replay};
use orrery_grader::GradeError;
use orrery_proto::{Budget, Role};
use orrery_session::SessionStore;

/// Says yes. The point of these tests is the numbers, not the grading.
struct AlwaysPasses;

#[async_trait]
impl Grader for AlwaysPasses {
    fn id(&self) -> &str {
        "always-passes"
    }

    async fn grade(&self, _input: GradeInput) -> Result<Score, GradeError> {
        Ok(Score::pass())
    }
}

fn suite() -> Suite {
    Suite::new("cost").with_case(EvalCase::new(
        "one-file",
        "write out.txt",
        GraderSpec::new("always-passes"),
    ))
}

fn three_roles(store: Arc<dyn SessionStore>) -> HarnessRunner {
    let mut bindings = HashMap::new();
    bindings.insert(
        Role::Planner,
        RoleBinding {
            provider: Arc::new(ScriptedProvider::from_fixture(
                "fixture",
                fixture("planner.jsonl"),
            )),
            model: "big".to_owned(),
        },
    );
    bindings.insert(
        Role::Executor,
        RoleBinding {
            provider: Arc::new(ScriptedProvider::from_fixture(
                "fixture",
                fixture("executor.jsonl"),
            )),
            model: "big".to_owned(),
        },
    );
    bindings.insert(
        Role::Compactor,
        RoleBinding {
            provider: Arc::new(ScriptedProvider::from_fixture(
                "fixture",
                fixture("compactor.jsonl"),
            )),
            model: "cheap".to_owned(),
        },
    );
    HarnessRunner::new("ours", store, bindings)
}

#[tokio::test]
async fn cost_comes_from_telemetry() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::default());
    let runner = Arc::new(three_roles(Arc::clone(&store)));
    let probe = Arc::clone(&runner);

    let report = orrery_eval::EvalRunner::new(Isolator::new(tmp.path()))
        .with_runner("review", runner)
        .with_grader(Arc::new(AlwaysPasses))
        .run(
            &EvalRun::new("cost", Matrix::new(["review"], ["m"])),
            &suite(),
        )
        .await
        .expect("the run");

    let result = &report.results[0];

    // The exact sum of the three committed fixtures, to the token and to the
    // micro-USD. An estimate could not land on these numbers.
    assert_eq!(result.cost.input_tokens, 120 + 300 + 50);
    assert_eq!(result.cost.output_tokens, 40 + 150 + 10);
    assert_eq!(result.cost.micro_usd, Some(900 + 2500 + 30));

    // And it is labelled as ours.
    assert_eq!(
        result.cost_provenance,
        CostProvenance::MeasuredAtProviderBoundary
    );

    // The proof that no estimation path exists: the provider's own token
    // counter is wired in behind a probe, and nothing asked it anything for the
    // whole run. A cost derived from an estimate would have had to.
    assert_eq!(
        probe.estimator_probe().questions(),
        0,
        "something consulted a token estimator during a run whose cost must come \
         from the provider boundary"
    );
}

#[tokio::test]
async fn by_role_attribution() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::default());
    let report = orrery_eval::EvalRunner::new(Isolator::new(tmp.path()))
        .with_runner("review", Arc::new(three_roles(Arc::clone(&store))))
        .with_grader(Arc::new(AlwaysPasses))
        .run(
            &EvalRun::new("cost", Matrix::new(["review"], ["m"])),
            &suite(),
        )
        .await
        .expect("the run");

    let by_role = &report.results[0].by_role;
    assert_eq!(by_role[&Role::Planner].usage.output_tokens, 40);
    assert_eq!(by_role[&Role::Executor].usage.output_tokens, 150);
    assert_eq!(by_role[&Role::Compactor].usage.output_tokens, 10);
    assert!(
        by_role[&Role::Compactor].usage.micro_usd < by_role[&Role::Planner].usage.micro_usd,
        "the cheap compactor should cost less than the planner"
    );
    for role in [Role::Planner, Role::Executor, Role::Compactor] {
        assert_eq!(by_role[&role].passes, 1);
    }
}

#[tokio::test]
async fn by_role_separates_even_on_one_model() {
    // Plan 16, open question 4: attribution is by step, not by model, so
    // binding every role to the same provider must still separate the numbers.
    let tmp = tempfile::tempdir().expect("tempdir");
    let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::default());
    let one: Arc<ScriptedProvider> = Arc::new(ScriptedProvider::from_fixture(
        "fixture",
        fixture("planner.jsonl"),
    ));
    let mut bindings = HashMap::new();
    for role in [Role::Planner, Role::Executor, Role::Compactor] {
        bindings.insert(
            role,
            RoleBinding {
                provider: Arc::clone(&one) as Arc<dyn orrery_provider::Provider>,
                model: "same".to_owned(),
            },
        );
    }

    let report = orrery_eval::EvalRunner::new(Isolator::new(tmp.path()))
        .with_runner(
            "review",
            Arc::new(HarnessRunner::new("ours", Arc::clone(&store), bindings)),
        )
        .with_grader(Arc::new(AlwaysPasses))
        .run(
            &EvalRun::new("cost", Matrix::new(["review"], ["m"])),
            &suite(),
        )
        .await
        .expect("the run");

    let by_role = &report.results[0].by_role;
    assert_eq!(by_role.len(), 3, "one line per role, not one per model");
    for role in [Role::Planner, Role::Executor, Role::Compactor] {
        assert_eq!(by_role[&role].usage.output_tokens, 40);
        assert_eq!(by_role[&role].passes, 1);
    }
    assert_eq!(report.results[0].cost.output_tokens, 120);
}

#[tokio::test]
async fn budget_exceeded_is_an_outcome() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::default());
    let suite = Suite::new("cost").with_case(
        EvalCase::new(
            "one-file",
            "write out.txt",
            GraderSpec::new("always-passes"),
        )
        .with_budget(Budget {
            max_turns: 0,
            max_tokens: 100,
            wall_clock_ms: 0,
            max_micro_usd: None,
        }),
    );

    let report = orrery_eval::EvalRunner::new(Isolator::new(tmp.path()))
        .with_runner("review", Arc::new(three_roles(Arc::clone(&store))))
        .with_grader(Arc::new(AlwaysPasses))
        .run(
            &EvalRun::new("cost", Matrix::new(["review"], ["m"])),
            &suite,
        )
        .await
        .expect("running out of budget is not a runner failure");

    assert_eq!(report.results[0].outcome, EvalOutcome::BudgetExceeded);
    assert_eq!(
        report.results[0].turns, 1,
        "it stopped after the first pass"
    );
}

#[tokio::test]
async fn transcript_is_replayable() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::default());
    let report = orrery_eval::EvalRunner::new(Isolator::new(tmp.path()))
        .with_runner("review", Arc::new(three_roles(Arc::clone(&store))))
        .with_grader(Arc::new(AlwaysPasses))
        .run(
            &EvalRun::new("cost", Matrix::new(["review"], ["m"])),
            &suite(),
        )
        .await
        .expect("the run");

    let replayed = replay(&store, &report, "one-file")
        .await
        .expect("the failing session opens");
    assert_eq!(replayed.case, "one-file");
    // The prompt, then one assistant turn per pass.
    assert_eq!(replayed.messages.len(), 4);
    assert_eq!(replayed.events.len(), 4);
    let text = format!("{:?}", replayed.messages);
    assert!(text.contains("write out.txt"), "the prompt is in there");
    assert!(text.contains("I will edit the file."), "so is the answer");
}

#[test]
fn defaults_are_reproducible() {
    let run = EvalRun::from_toml(
        r#"
        suite = "swebench-lite"
        [matrix]
        profiles = ["review", "fast"]
        "#,
    )
    .expect("a run with no memory or router keys");

    assert_eq!(run.memory, MemoryMode::Off);
    assert_eq!(run.router, RouterMode::Declared);
    assert_eq!(run.concurrency, 1);
    assert!(run.reproducibility().is_pinned());
}

#[test]
fn a_declared_memory_provider_is_not_pinned() {
    let run = EvalRun::from_toml(
        r#"
        suite = "s"
        [memory]
        mode = "provider"
        id = "file"
        "#,
    )
    .expect("a run that declares memory");
    assert!(!run.reproducibility().is_pinned());
}

#[test]
fn matrix_expands() {
    let points = Matrix::new(["review", "fast"], ["sonnet", "qwen"])
        .with_seeds([1, 2])
        .expand();

    assert_eq!(points.len(), 8);
    // Profile-major, then model, then seed — and the order is the contract.
    let labels: Vec<String> = points.iter().map(|p| p.label()).collect();
    assert_eq!(
        labels,
        vec![
            "review/sonnet@1",
            "review/sonnet@2",
            "review/qwen@1",
            "review/qwen@2",
            "fast/sonnet@1",
            "fast/sonnet@2",
            "fast/qwen@1",
            "fast/qwen@2",
        ]
    );
}

#[test]
fn an_empty_axis_is_one_point_not_none() {
    assert_eq!(Matrix::default().expand().len(), 1);
}

#[tokio::test]
async fn an_unbound_profile_is_refused_before_anything_runs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let err = EvalRunner::new(Isolator::new(tmp.path()))
        .with_grader(Arc::new(AlwaysPasses))
        .run(
            &EvalRun::new("cost", Matrix::new(["nobody"], ["m"])),
            &suite(),
        )
        .await
        .expect_err("no runner is bound");
    assert!(err.to_string().contains("nobody"));
}
