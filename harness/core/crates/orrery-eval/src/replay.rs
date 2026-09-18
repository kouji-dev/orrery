//! Opening the exact session a case produced.
//!
//! This is what makes a failed case actionable rather than a red square, and it
//! works for the reason plan 02 built the turn tree the way it did: turns are
//! immutable and sequence numbers are contiguous, so "the session that failed"
//! is a thing that still exists and can be read back byte for byte.

use std::sync::Arc;

use orrery_session::turn::{StoredEvent, TurnRow};
use orrery_session::{SessionError, SessionStore};

use crate::error::EvalError;
use crate::report::{EvalResult, RunReport};

/// A case's session, as the store kept it.
#[derive(Clone, Debug, PartialEq)]
pub struct Replay {
    /// Which case.
    pub case: String,
    /// Which profile it ran under.
    pub profile: String,
    /// Every event, oldest first, contiguous.
    pub events: Vec<StoredEvent>,
    /// The messages that were actually in play, oldest first.
    pub messages: Vec<orrery_proto::Message>,
}

impl Replay {
    /// How many turns the session holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether it holds nothing at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// Re-open one case of a finished run.
///
/// # Errors
///
/// [`EvalError::NoGrader`] is never returned here; what can fail is the store,
/// or the case not being in the report.
pub async fn replay(
    store: &Arc<dyn SessionStore>,
    report: &RunReport,
    case: &str,
) -> Result<Replay, EvalError> {
    let result = report
        .results
        .iter()
        .find(|r| r.case == case)
        .ok_or_else(|| EvalError::Parse {
            what: "run report",
            detail: format!("run `{}` has no case `{case}`", report.run_id),
        })?;
    replay_result(store, result).await
}

/// Re-open one result's session.
///
/// # Errors
///
/// Whatever the session store says.
pub async fn replay_result(
    store: &Arc<dyn SessionStore>,
    result: &EvalResult,
) -> Result<Replay, EvalError> {
    let events = store.events_since(result.transcript.session, None).await?;
    let view = store
        .materialise(
            result.transcript.branch,
            orrery_proto::TokenBudget {
                max: u64::MAX,
                reserve: 0,
            },
            &orrery_session::algebra::CharsOverFour,
        )
        .await?;
    Ok(Replay {
        case: result.case.clone(),
        profile: result.profile.clone(),
        events,
        messages: view.messages,
    })
}

/// The rows of a replay, for a caller that wants the tree rather than the
/// stream. Kept as a separate function because most callers want the events.
///
/// # Errors
///
/// [`SessionError`] when the branch is gone.
pub fn rows_of(view: &[TurnRow], branch: orrery_proto::BranchId) -> Result<Vec<TurnRow>, SessionError> {
    Ok(view.iter().filter(|r| r.branch == branch).cloned().collect())
}
