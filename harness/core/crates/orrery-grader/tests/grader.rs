//! The published trait's two promises: it is object-safe, and a grader can be
//! written without naming the runner.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use orrery_grader::{CaseRef, EvalOutcome, GradeError, GradeInput, Grader, Score};
use orrery_proto::{BranchId, SessionId, SessionRef, SurfaceKind};

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

fn input() -> GradeInput {
    GradeInput::new(
        PathBuf::from("."),
        SessionRef {
            session: SessionId::new(),
            branch: BranchId::new(),
            turn: None,
        },
        CaseRef::new("suite", "case"),
    )
}

#[test]
fn a_config_a_grader_cannot_read_is_a_misconfiguration_not_a_failure() {
    #[derive(Debug, Default, serde::Deserialize)]
    struct Checks {
        #[allow(dead_code)]
        checks: Vec<String>,
    }

    // No config at all is the grader's own default, not an error.
    assert!(input().parse_config::<Checks>("g").is_ok());

    let wrong = input().with_config(serde_json::json!({ "checks": 7 }));
    let err = wrong
        .parse_config::<Checks>("g")
        .expect_err("7 is not a list");
    assert_eq!(err.grader(), "g");
    assert!(err.to_string().contains("misconfigured"));
}

#[test]
fn is_object_safe() {
    // The whole reason this crate exists: a runner holds `Arc<dyn Grader>`, so
    // a grader extension is selected at runtime and never linked by name.
    let graders: Vec<Arc<dyn Grader>> = vec![Arc::new(AlwaysPasses)];
    assert_eq!(graders[0].id(), "always-passes");
}

#[tokio::test]
async fn a_minimal_grader_scores() {
    let score = AlwaysPasses.grade(input()).await.expect("graded");
    assert_eq!(score.outcome, EvalOutcome::Pass);
    assert!(
        score.judge_cost.is_none(),
        "a non-model grader costs nothing"
    );
}

#[test]
fn a_score_carries_its_detail() {
    let score = Score::fail("exit code 1");
    assert_eq!(score.outcome, EvalOutcome::Fail);
    assert!(matches!(score.detail.kind, SurfaceKind::Text { .. }));
    assert!(!score.outcome.is_pass());
}
