//! `model` — a judge model with a rubric, whose own cost is counted.
//!
//! # The judge's spend is not the run's spend
//!
//! A judge that costs more than the run it grades should be visible, so its
//! usage goes into [`Score::judge_cost`] and never into the run's cost. The
//! number is read exactly the way the runner reads its own: summed from
//! [`ModelEvent::Usage`] at the provider boundary, with nothing estimated. The
//! grader has no way to report a cost it did not observe, because the only
//! thing it adds up is what the provider emitted.

use std::sync::Arc;

use async_trait::async_trait;
use futures_util::StreamExt as _;
use orrery_grader::{EvalOutcome, GradeError, GradeInput, Grader, Score};
use orrery_proto::{ContentBlock, Message, MessageRole, Usage};
use orrery_provider::{ModelEvent, ModelRequest, Provider};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// The id a case selects this grader by.
pub const ID: &str = "model";

/// What a case tells the model grader.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelConfig {
    /// What the judge is asked to look for.
    #[serde(default)]
    pub rubric: String,
    /// The score at or above which the case passes.
    #[serde(default = "half")]
    pub pass_at: f64,
    /// How much of the workspace summary to send. Judges are not free.
    #[serde(default = "default_max_output")]
    pub max_output_tokens: u64,
}

fn half() -> f64 {
    0.5
}

const fn default_max_output() -> u64 {
    1024
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            rubric: String::new(),
            pass_at: half(),
            max_output_tokens: default_max_output(),
        }
    }
}

/// What a judge is expected to answer with.
///
/// A judge that answers with prose instead is read leniently — see
/// [`parse_verdict`] — because a rubric that fails on formatting grades the
/// model's JSON, not the case.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
struct Verdict {
    #[serde(default)]
    verdict: String,
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    why: String,
}

/// A judge model behind the [`Grader`] trait.
pub struct ModelGrader {
    provider: Arc<dyn Provider>,
    model: String,
}

impl ModelGrader {
    /// A judge over one provider and one model.
    ///
    /// The provider is bound by the runner, not chosen by the case: a suite
    /// that could pick its own judge could pick a generous one.
    #[must_use]
    pub fn new(provider: Arc<dyn Provider>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
        }
    }
}

#[async_trait]
impl Grader for ModelGrader {
    fn id(&self) -> &str {
        ID
    }

    async fn grade(&self, input: GradeInput) -> Result<Score, GradeError> {
        let config: ModelConfig = input.parse_config(ID)?;
        if config.rubric.trim().is_empty() {
            return Err(GradeError::Misconfigured {
                grader: ID.to_owned(),
                detail: "no rubric: a judge with nothing to judge against invents one".to_owned(),
            });
        }

        let prompt = format!(
            "Grade case `{}` of suite `{}`, in workspace `{}`.\n\nRubric:\n{}\n\n\
             Answer with JSON: {{\"verdict\":\"pass\"|\"fail\",\"score\":0.0-1.0,\"why\":\"…\"}}",
            input.case.case,
            input.case.suite,
            input.workspace.display(),
            config.rubric,
        );
        let request = ModelRequest::new(
            self.model.clone(),
            Arc::from(
                vec![Message {
                    role: MessageRole::User,
                    content: vec![ContentBlock::Text { text: prompt }],
                }]
                .into_boxed_slice(),
            ),
            config.max_output_tokens,
        );

        let mut stream = self.provider.stream(request, CancellationToken::new());
        let mut text = String::new();
        // The judge's own spend, summed from what the provider emitted. There
        // is no other source for this number.
        let mut cost = Usage::default();
        while let Some(event) = stream.next().await {
            let event = event.map_err(|e| GradeError::Failed {
                grader: ID.to_owned(),
                detail: e.to_string(),
            })?;
            match event {
                ModelEvent::TextDelta { text: chunk } => text.push_str(&chunk),
                ModelEvent::Usage { usage } => cost += usage,
                _ => {}
            }
        }

        let verdict = parse_verdict(&text);
        let score = verdict
            .score
            .unwrap_or(if verdict.verdict == "pass" { 1.0 } else { 0.0 });
        let outcome = if score >= config.pass_at {
            EvalOutcome::Pass
        } else {
            EvalOutcome::Fail
        };
        let why = if verdict.why.trim().is_empty() {
            text.trim().to_owned()
        } else {
            verdict.why.clone()
        };

        Ok(Score::new(outcome, format!("judge: {why}"))
            .with_score(score)
            .with_judge_cost(cost))
    }
}

/// Read a verdict out of whatever the judge said.
///
/// The JSON object first, then the last JSON-looking span in the text, then the
/// words `pass` and `fail`. A judge that wrapped its answer in prose has still
/// answered.
fn parse_verdict(text: &str) -> Verdict {
    if let Ok(v) = serde_json::from_str::<Verdict>(text.trim()) {
        return v;
    }
    if let (Some(start), Some(end)) = (text.find('{'), text.rfind('}')) {
        if start < end {
            if let Ok(v) = serde_json::from_str::<Verdict>(&text[start..=end]) {
                return v;
            }
        }
    }
    let lower = text.to_lowercase();
    let verdict = if lower.contains("fail") {
        "fail"
    } else if lower.contains("pass") {
        "pass"
    } else {
        ""
    };
    Verdict {
        verdict: verdict.to_owned(),
        score: None,
        why: text.trim().to_owned(),
    }
}
