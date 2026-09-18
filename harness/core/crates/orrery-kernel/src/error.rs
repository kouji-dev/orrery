//! What can go wrong that is *not* an ending.
//!
//! # There is no budget variant here, and there never will be
//!
//! A ceiling reached, a cancellation, a refusal and a login prompt are all
//! [`TurnOutcome`](crate::TurnOutcome) values. `tests/budget.rs` asserts the
//! absence of a `StoppedByBudget` variant on this type, so the rule is checked
//! rather than promised. What is left is the cases where the harness itself
//! broke: the store would not answer, the registry could not carry a call.

use orrery_session::SessionError;
use orrery_tools::ToolError;

/// The harness could not run the turn.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    /// The session store failed. §4.7 makes it the one singleton that cannot
    /// degrade, so this ends the turn and usually the session.
    #[error(transparent)]
    Session(#[from] SessionError),

    /// A tool call could not be carried at all — an unusable schema, an
    /// unreachable host. **Not** a refusal: that is `Outcome::Denied`.
    #[error(transparent)]
    Tool(#[from] ToolError),

    /// An interceptor could not be registered.
    #[error(transparent)]
    Register(#[from] crate::intercept::RegisterError),
}
