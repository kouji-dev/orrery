//! Task 6 and task 9: comparing runs, replaying a case, and the CI exit code.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use common::store::MemoryStore;
use common::{ScriptedProvider, fixture};
use orrery_eval::case::{EvalCase, GraderSpec, Suite};
use orrery_eval::compare::{CompareError, compare};
use orrery_eval::junit::{Baseline, exit_code, junit_xml};
use orrery_eval::matrix::Matrix;
use orrery_eval::report::{CostProvenance, EvalResult, RunReport, Timing};
use orrery_eval::run::{EvalRun, HarnessRunner, MemoryMode, RoleBinding};
use orrery_eval::{EvalOutcome, EvalRunner, GradeInput, Grader, Isolator, Score, replay};
use orrery_grader::GradeError;
use orrery_proto::{BranchId, Role, SessionId, SessionRef, Usage};
use orrery_session::SessionStore;

fn result(case: &str, outcome: EvalOutcome, tokens: u64) -> EvalResult {
    EvalResult {
        case: case.to_owned(),
        profile: "review".to_owned(),
        model: "m".to_owned(),
        seed: None,
        outcome,
        score: None,
        cost: Usage {
            input_tokens: tokens,
            output_tokens: 0,
            cache_hits: 0,
            micro_usd: Some(tokens),
        },
        cost_provenance: CostProvenance::MeasuredAtProviderBoundary,
        judge_cost: None,
        by_role: orrery_eval::report::ByRole::new(),
        timing: Timing::default(),
        turns: 1,
        tool_calls: 0,
        transcript: SessionRef {
            session: SessionId::new(),
            branch: BranchId::new(),
            turn: None,
        },
        detail: None,
    }
}

fn report(id: &str, results: Vec<EvalResult>) -> RunReport {
    RunReport {
        run_id: id.to_owned(),
        suite: "swebench-lite".to_owned(),
        reproducibility: orrery_eval::Reproducibility::default(),
        results,
    }
}

#[test]
fn refuses_incomparable_runs() {
    let a = report("run-812", vec![result("api-42", EvalOutcome::Pass, 10)]);
    let mut b = report("run-819", vec![result("api-42", EvalOutcome::Pass, 10)]);
    b.reproducibility.memory = MemoryMode::Provider {
        id: "file".to_owned(),
        scopes: vec!["project".to_owned()],
    };

    let err = compare(&a, &b, false).expect_err("memory differs");
    let message = err.to_string();
    assert!(matches!(err, CompareError::Incomparable { .. }));
    assert!(message.contains("memory"), "the message says why: {message}");
    assert!(message.contains("off") && message.contains("file"));
    assert!(message.contains("--force"), "and how to override it");

    let forced = compare(&a, &b, true).expect("--force compares anyway");
    assert!(forced.forced, "and the comparison remembers that it was forced");
}

#[test]
fn reports_regressions() {
    let before = report(
        "run-812",
        vec![
            result("api-42", EvalOutcome::Pass, 100),
            result("api-43", EvalOutcome::Fail, 100),
            result("api-44", EvalOutcome::Pass, 100),
        ],
    );
    let after = report(
        "run-819",
        vec![
            result("api-42", EvalOutcome::Fail, 160),
            result("api-43", EvalOutcome::Pass, 100),
            result("api-44", EvalOutcome::Pass, 100),
        ],
    );

    let diff = compare(&before, &after, false).expect("comparable");
    assert!(diff.has_regressions());
    assert_eq!(diff.regressions.len(), 1);
    assert_eq!(diff.regressions[0].key, "api-42·review/m");
    assert_eq!(diff.improvements.len(), 1);
    assert_eq!(diff.token_delta, 60);
    assert!(diff.render().contains("REGRESSED"));
}

#[test]
fn a_case_only_one_run_has_is_unmatched_not_a_regression() {
    let before = report("a", vec![result("kept", EvalOutcome::Pass, 1)]);
    let after = report(
        "b",
        vec![
            result("kept", EvalOutcome::Pass, 1),
            result("new", EvalOutcome::Fail, 1),
        ],
    );
    let diff = compare(&before, &after, false).expect("comparable");
    assert!(!diff.has_regressions());
    assert_eq!(diff.unmatched, vec!["new·review/m".to_owned()]);
}

#[test]
fn two_suites_never_compare() {
    let a = report("a", vec![]);
    let mut b = report("b", vec![]);
    b.suite = "other".to_owned();
    assert!(matches!(
        compare(&a, &b, true).expect_err("not even with --force"),
        CompareError::DifferentSuites { .. }
    ));
}

#[test]
fn exits_non_zero_on_regression() {
    let baseline = Baseline::from_report(&report(
        "run-812",
        vec![
            result("api-42", EvalOutcome::Pass, 10),
            result("api-43", EvalOutcome::Fail, 10),
        ],
    ));
    // Round-trips through the committed file it lives in.
    let baseline =
        Baseline::from_json(&baseline.to_json().expect("serialise")).expect("deserialise");

    let green = report(
        "run-819",
        vec![
            result("api-42", EvalOutcome::Pass, 10),
            result("api-43", EvalOutcome::Fail, 10),
        ],
    );
    assert_eq!(
        exit_code(&green, Some(&baseline)),
        0,
        "a case that was already failing is not a regression"
    );

    let red = report(
        "run-820",
        vec![
            result("api-42", EvalOutcome::Fail, 10),
            result("api-43", EvalOutcome::Fail, 10),
        ],
    );
    assert_eq!(exit_code(&red, Some(&baseline)), 1);
    assert_eq!(baseline.regressions(&red), vec!["api-42·review/m".to_owned()]);

    // With no baseline at all, any failure is non-zero: "no baseline" is not a
    // reason to call a red suite green.
    assert_eq!(exit_code(&red, None), 1);
    assert_eq!(exit_code(&green, None), 1);
}

#[test]
fn junit_carries_the_provenance() {
    let mut r = report("run-1", vec![result("api-42", EvalOutcome::Fail, 10)]);
    r.results[0].cost_provenance = CostProvenance::ReportedByTool {
        tool: "codex".to_owned(),
    };
    let xml = junit_xml(&r);
    assert!(xml.contains("<testsuite name=\"swebench-lite\""));
    assert!(xml.contains("failures=\"1\""));
    assert!(
        xml.contains("reported by codex"),
        "the CI artefact must not lose the provenance: {xml}"
    );
}

#[test]
fn a_report_round_trips_through_json() {
    let r = report("run-1", vec![result("api-42", EvalOutcome::Pass, 10)]);
    let back = RunReport::from_json(&r.to_json().expect("serialise")).expect("deserialise");
    assert_eq!(back, r);
}

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

#[tokio::test]
async fn replay_opens_the_failing_session() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let store: Arc<dyn SessionStore> = Arc::new(MemoryStore::default());
    let mut bindings = HashMap::new();
    bindings.insert(
        Role::Planner,
        RoleBinding {
            provider: Arc::new(ScriptedProvider::from_fixture(
                "fixture",
                fixture("planner.jsonl"),
            )),
            model: "m".to_owned(),
        },
    );

    let suite = Suite::new("s")
        .with_case(EvalCase::new("api-42", "fix the thing", GraderSpec::new("always-passes")))
        .with_case(EvalCase::new("api-43", "fix the other thing", GraderSpec::new("always-passes")));

    let report = EvalRunner::new(Isolator::new(tmp.path()))
        .with_runner(
            "review",
            Arc::new(HarnessRunner::new("ours", Arc::clone(&store), bindings)),
        )
        .with_grader(Arc::new(AlwaysPasses))
        .with_grader(Arc::new(AlwaysPasses))
        .run(&EvalRun::new("s", Matrix::new(["review"], ["m"])), &suite)
        .await
        .expect("the run");

    let replayed = replay(&store, &report, "api-42").await.expect("replayed");
    assert_eq!(replayed.case, "api-42");
    let text = format!("{:?}", replayed.messages);
    assert!(text.contains("fix the thing"));
    assert!(
        !text.contains("fix the other thing"),
        "replay opened the wrong case's session"
    );

    // And the recorded usage is the run's usage, turn for turn.
    let recorded: u64 = replayed
        .events
        .iter()
        .map(|e| match &e.event {
            orrery_proto::Event::TurnSettled { usage, .. } => usage.total_tokens(),
            _ => 0,
        })
        .sum();
    let result = report.result("api-42", "review").expect("the result");
    assert_eq!(recorded, result.cost.total_tokens());

    assert!(
        replay(&store, &report, "nope").await.is_err(),
        "a case that is not in the run cannot be replayed"
    );
}
