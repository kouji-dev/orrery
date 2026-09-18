//! `command` — the exit code is the score.
//!
//! The obvious grader for a SWE-style patch benchmark: run the tests, and if
//! they pass, the patch is good.
//!
//! # It is brokered like anything else
//!
//! The script does **not** go through `std::process::Command`. It goes through
//! [`BrokerFacade::spawn`], under this extension's `spawn` grant, with the
//! call's own timeout and output ceiling. A grader is somebody else's code
//! running a command line out of somebody else's suite file; the one thing it
//! must not be is the one process in the system that is trusted.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use orrery_ext_api::{BrokerFacade, SpawnRequest};
use orrery_grader::{EvalOutcome, GradeError, GradeInput, Grader, Score};
use serde::{Deserialize, Serialize};

/// The id a case selects this grader by.
pub const ID: &str = "command";

/// What a case tells the command grader.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandConfig {
    /// The command line, program first. Split on whitespace.
    #[serde(default)]
    pub command: String,
    /// Arguments, when the case would rather not put them in `command`.
    #[serde(default)]
    pub args: Vec<String>,
    /// Where to run it. Relative paths are taken against the workspace;
    /// `None` is the workspace itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
    /// How long it may take.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Exit codes to count as a pass. Empty means `[0]`, which is the whole
    /// convention — but a suite whose test runner exits 2 for "no tests" can
    /// say so rather than lying about what passed.
    #[serde(default)]
    pub pass_on: Vec<i32>,
}

impl CommandConfig {
    /// The program and its arguments, as the broker wants them.
    fn argv(&self) -> Option<(String, Vec<String>)> {
        let mut parts = self.command.split_whitespace().map(str::to_owned);
        let program = parts.next()?;
        let mut args: Vec<String> = parts.collect();
        args.extend(self.args.iter().cloned());
        Some((program, args))
    }

    fn passes(&self, status: Option<i32>) -> bool {
        let accepted: &[i32] = if self.pass_on.is_empty() {
            &[0]
        } else {
            &self.pass_on
        };
        status.is_some_and(|s| accepted.contains(&s))
    }
}

/// Runs a script and reads its exit code.
pub struct CommandGrader {
    broker: Arc<dyn BrokerFacade>,
}

impl CommandGrader {
    /// A grader over the broker this extension was given.
    #[must_use]
    pub fn new(broker: Arc<dyn BrokerFacade>) -> Self {
        Self { broker }
    }
}

#[async_trait]
impl Grader for CommandGrader {
    fn id(&self) -> &str {
        ID
    }

    async fn grade(&self, input: GradeInput) -> Result<Score, GradeError> {
        let config: CommandConfig = input.parse_config(ID)?;
        let (program, args) = config.argv().ok_or_else(|| GradeError::Misconfigured {
            grader: ID.to_owned(),
            detail: "no `command` to run".to_owned(),
        })?;

        let cwd = match &config.cwd {
            Some(dir) if dir.is_absolute() => dir.clone(),
            Some(dir) => input.workspace.join(dir),
            None => input.workspace.clone(),
        };
        let mut request = SpawnRequest::new(&program, args).in_dir(cwd);
        request.timeout_ms = config.timeout_ms;

        // A denial is `Unavailable`, not `Fail`: the grader did not decide that
        // the case is bad, it never got to look.
        let output = self
            .broker
            .spawn(request)
            .await
            .map_err(|e| GradeError::Unavailable {
                grader: ID.to_owned(),
                detail: e.to_string(),
            })?;

        let status = output.status;
        let tail = String::from_utf8_lossy(&output.stderr);
        let tail = if tail.trim().is_empty() {
            String::from_utf8_lossy(&output.stdout).into_owned()
        } else {
            tail.into_owned()
        };
        let detail = format!(
            "`{program}` exited {}{}",
            status.map_or_else(|| "without a code".to_owned(), |s| s.to_string()),
            last_line(&tail).map_or_else(String::new, |l| format!(": {l}")),
        );

        Ok(if config.passes(status) {
            Score::new(EvalOutcome::Pass, detail).with_score(1.0)
        } else {
            Score::new(EvalOutcome::Fail, detail).with_score(0.0)
        })
    }
}

/// The last non-empty line of the output, which is where a test runner puts its
/// summary.
fn last_line(text: &str) -> Option<&str> {
    text.lines().rev().map(str::trim).find(|l| !l.is_empty())
}
