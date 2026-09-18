//! Task 5: the three graders.
//!
//! **No model is ever called.** The judge is a scripted provider replaying
//! hand-written events, which is also what makes `counts_its_own_cost` an exact
//! assertion rather than an approximate one.

use std::sync::Arc;

use async_trait::async_trait;
use futures_core::stream::BoxStream;
use orrery_ext_api::testing::{BrokerCall, MockBroker};
use orrery_ext_api::{BrokerFacade, BrokerResult, SpawnOutput, SpawnRequest};
use orrery_ext_graders::{
    AssertionGrader, Check, CommandGrader, EvalOutcome, GradeError, GradeInput, ModelGrader,
};
use orrery_grader::{CaseRef, Grader};
use orrery_proto::{Aspect, BranchId, Capability, SessionId, SessionRef, Usage};
use orrery_provider::{
    AuthCtx, AuthMethod, AuthState, Capabilities, HeuristicCounter, ModelEvent, ModelRequest,
    Provider, ProviderAuth, ProviderError, TokenCounter,
};
use tokio_util::sync::CancellationToken;

fn input(config: serde_json::Value) -> GradeInput {
    GradeInput::new(
        std::path::PathBuf::from("/ws"),
        SessionRef {
            session: SessionId::new(),
            branch: BranchId::new(),
            turn: None,
        },
        CaseRef::new("suite", "case"),
    )
    .with_config(config)
}

/// A broker that answers with exactly the exit code a test asks for.
///
/// `MockBroker` only ever hands back status 0, and the point of the command
/// grader is the code it was given.
struct ExitsWith {
    status: Option<i32>,
    stdout: &'static str,
}

#[async_trait]
impl BrokerFacade for ExitsWith {
    async fn spawn(&self, _req: SpawnRequest) -> BrokerResult<SpawnOutput> {
        Ok(SpawnOutput {
            status: self.status,
            stdout: self.stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
            truncated: false,
        })
    }
}

#[tokio::test]
async fn exit_code_is_the_score() {
    let green = CommandGrader::new(Arc::new(ExitsWith {
        status: Some(0),
        stdout: "test result: ok. 12 passed",
    }));
    let score = green
        .grade(input(serde_json::json!({ "command": "cargo test" })))
        .await
        .expect("graded");
    assert_eq!(score.outcome, EvalOutcome::Pass);
    assert_eq!(score.score, Some(1.0));
    assert!(score.judge_cost.is_none(), "a command grader costs nothing");

    let red = CommandGrader::new(Arc::new(ExitsWith {
        status: Some(101),
        stdout: "test result: FAILED. 1 failed",
    }));
    let score = red
        .grade(input(serde_json::json!({ "command": "cargo test" })))
        .await
        .expect("graded");
    assert_eq!(score.outcome, EvalOutcome::Fail);
    assert_eq!(score.score, Some(0.0));
    let detail = format!("{:?}", score.detail);
    assert!(
        detail.contains("101"),
        "the code is in the detail: {detail}"
    );
    assert!(detail.contains("FAILED"), "and so is the last line");
}

#[tokio::test]
async fn a_runner_that_exits_two_can_be_declared_a_pass() {
    let grader = CommandGrader::new(Arc::new(ExitsWith {
        status: Some(2),
        stdout: "no tests ran",
    }));
    let score = grader
        .grade(input(
            serde_json::json!({ "command": "ctest", "pass_on": [0, 2] }),
        ))
        .await
        .expect("graded");
    assert_eq!(score.outcome, EvalOutcome::Pass);
}

#[tokio::test]
async fn runs_under_a_grant() {
    // Granted: the spawn goes through, and the broker recorded it.
    let broker = Arc::new(MockBroker::new(vec![Capability {
        aspect: Aspect::Spawn,
        scope: vec!["cargo".to_owned()],
    }]));
    broker.add_spawn_response("cargo", "ok");
    let score = CommandGrader::new(Arc::clone(&broker) as Arc<dyn BrokerFacade>)
        .grade(input(serde_json::json!({ "command": "cargo test" })))
        .await
        .expect("graded");
    assert_eq!(score.outcome, EvalOutcome::Pass);
    assert!(
        broker.recorded().iter().any(|c| matches!(
            c,
            BrokerCall::Spawn { program, allowed: true, .. } if program == "cargo"
        )),
        "the grader script went through the broker, not around it"
    );

    // Not granted: the grader cannot decide, and says so. It does **not**
    // report the case as failing — the case was never looked at.
    let ungranted = Arc::new(MockBroker::new(vec![]));
    let err = CommandGrader::new(ungranted)
        .grade(input(serde_json::json!({ "command": "cargo test" })))
        .await
        .expect_err("no spawn grant");
    assert!(matches!(err, GradeError::Unavailable { .. }));
    assert!(err.to_string().contains("denied"), "{err}");
}

#[tokio::test]
async fn checks_files_diffs_and_tool_calls() {
    let broker = Arc::new(MockBroker::new(vec![
        Capability {
            aspect: Aspect::Read,
            scope: vec![],
        },
        Capability {
            aspect: Aspect::Spawn,
            scope: vec!["git".to_owned()],
        },
    ]));
    broker.add_file(
        std::path::PathBuf::from("/ws").join("src/lib.rs"),
        "pub fn parse() {}",
    );
    // What `git status --porcelain` prints: two columns, then the path.
    broker.add_spawn_response("git", " M src/lib.rs\n?? notes.txt\n");

    let grader = AssertionGrader::new(Arc::clone(&broker) as Arc<dyn BrokerFacade>);

    let held = grader
        .grade(input(serde_json::json!({ "checks": [
            { "kind": "file-exists", "path": "src/lib.rs" },
            { "kind": "file-contains", "path": "src/lib.rs", "text": "pub fn parse" },
            { "kind": "file-absent", "path": "src/gone.rs" },
            { "kind": "file-does-not-contain", "path": "src/lib.rs", "text": "unsafe" },
            { "kind": "diff-touches", "path": "src/" },
            { "kind": "diff-does-not-touch", "path": ".github/" },
        ] })))
        .await
        .expect("graded");
    assert_eq!(held.outcome, EvalOutcome::Pass, "{:?}", held.detail);
    assert_eq!(held.score, Some(1.0));

    let broke = grader
        .grade(input(serde_json::json!({ "checks": [
            { "kind": "file-exists", "path": "src/lib.rs" },
            { "kind": "diff-does-not-touch", "path": "src/" },
        ] })))
        .await
        .expect("graded");
    assert_eq!(broke.outcome, EvalOutcome::Fail);
    // A fraction, not a flag: half the checks held.
    assert_eq!(broke.score, Some(0.5));
    let detail = format!("{:?}", broke.detail);
    assert!(
        detail.contains("the diff leaves src/ alone"),
        "the failure names the check that did not hold: {detail}"
    );
}

#[tokio::test]
async fn a_tool_call_check_without_a_transcript_is_unavailable_not_a_pass() {
    // A safety assertion that cannot be evaluated must never look like one that
    // held.
    let broker = Arc::new(MockBroker::new(vec![]));
    let err = AssertionGrader::new(broker)
        .grade(input(serde_json::json!({ "checks": [
            { "kind": "tool-not-called", "name": "builtin.net" },
        ] })))
        .await
        .expect_err("no session store is bound");
    assert!(matches!(err, GradeError::Unavailable { .. }));
    assert!(err.to_string().contains("session store"));
}

#[tokio::test]
async fn an_assertion_grader_with_no_checks_is_a_misconfiguration() {
    let err = AssertionGrader::new(Arc::new(MockBroker::new(vec![])))
        .grade(input(serde_json::json!({ "checks": [] })))
        .await
        .expect_err("nothing to assert");
    assert!(matches!(err, GradeError::Misconfigured { .. }));
}

#[test]
fn a_check_describes_itself() {
    assert_eq!(
        Check::ToolNotCalled {
            name: "builtin.net".to_owned()
        }
        .describe(),
        "builtin.net was not called"
    );
    assert!(
        Check::ToolCalled {
            name: "x".to_owned()
        }
        .needs_transcript()
    );
    assert!(!Check::FileExists { path: "a".into() }.needs_transcript());
}

/// Replays hand-written judge events. No model, no key, no network.
struct ScriptedJudge(Vec<ModelEvent>);

impl Provider for ScriptedJudge {
    fn id(&self) -> &str {
        "scripted-judge"
    }

    fn capabilities(&self) -> &Capabilities {
        const CAPS: Capabilities = Capabilities {
            tools: false,
            images: false,
            cache: false,
            max_context: 100_000,
            max_output: 4096,
        };
        &CAPS
    }

    fn stream(
        &self,
        _req: ModelRequest,
        _cancel: CancellationToken,
    ) -> BoxStream<'static, Result<ModelEvent, ProviderError>> {
        let events = self.0.clone();
        Box::pin(futures_util::stream::iter(events.into_iter().map(Ok)))
    }

    fn counter(&self) -> Arc<dyn TokenCounter> {
        Arc::new(HeuristicCounter::new())
    }

    fn auth(&self) -> Arc<dyn ProviderAuth> {
        Arc::new(NoAuth)
    }
}

struct NoAuth;

#[async_trait]
impl ProviderAuth for NoAuth {
    fn methods(&self) -> &[AuthMethod] {
        &[]
    }

    async fn state(&self) -> Result<AuthState, ProviderError> {
        Ok(AuthState::Anonymous)
    }

    async fn login(&self, _ctx: &dyn AuthCtx) -> Result<AuthState, ProviderError> {
        Ok(AuthState::Anonymous)
    }

    async fn refresh(&self) -> Result<AuthState, ProviderError> {
        Ok(AuthState::Anonymous)
    }

    async fn logout(&self) -> Result<(), ProviderError> {
        Ok(())
    }
}

#[tokio::test]
async fn counts_its_own_cost() {
    let judge = ScriptedJudge(vec![
        ModelEvent::TextDelta {
            text: "{\"verdict\":\"pass\",\"score\":0.9,\"why\":\"it reads well\"}".to_owned(),
        },
        ModelEvent::Usage {
            usage: Usage {
                input_tokens: 4_000,
                output_tokens: 60,
                cache_hits: 0,
                micro_usd: Some(30_000),
            },
        },
        ModelEvent::Done {
            stop: orrery_provider::StopReason::EndTurn,
        },
    ]);

    let score = ModelGrader::new(Arc::new(judge), "judge-model")
        .grade(input(serde_json::json!({
            "rubric": "Does the patch explain itself?",
            "pass_at": 0.7,
        })))
        .await
        .expect("graded");

    assert_eq!(score.outcome, EvalOutcome::Pass);
    assert_eq!(score.score, Some(0.9));

    // The judge's spend is reported, and separately: it is `judge_cost`, and
    // the run's own cost is somewhere else entirely.
    let cost = score.judge_cost.expect("the judge's own cost is counted");
    assert_eq!(cost.input_tokens, 4_000);
    assert_eq!(cost.output_tokens, 60);
    assert_eq!(cost.micro_usd, Some(30_000));
}

#[tokio::test]
async fn a_judge_that_answers_in_prose_still_answers() {
    let judge = ScriptedJudge(vec![
        ModelEvent::TextDelta {
            text: "On reflection this should fail: the patch deletes the test.".to_owned(),
        },
        ModelEvent::Done {
            stop: orrery_provider::StopReason::EndTurn,
        },
    ]);
    let score = ModelGrader::new(Arc::new(judge), "judge-model")
        .grade(input(
            serde_json::json!({ "rubric": "Is the patch honest?" }),
        ))
        .await
        .expect("graded");
    assert_eq!(score.outcome, EvalOutcome::Fail);
    // Nothing was reported, so nothing is claimed.
    assert_eq!(score.judge_cost, Some(Usage::default()));
}

#[tokio::test]
async fn a_judge_with_no_rubric_is_a_misconfiguration() {
    let judge = ScriptedJudge(vec![]);
    let err = ModelGrader::new(Arc::new(judge), "judge-model")
        .grade(input(serde_json::json!({})))
        .await
        .expect_err("no rubric");
    assert!(matches!(err, GradeError::Misconfigured { .. }));
}
