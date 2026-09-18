//! Process exit codes. A CI script's response to 3, 4 and 5 is different, so
//! they are distinct (`17-cli.md`, exit-code table).

use orrery_kernel::TurnOutcome;
use orrery_proto::Outcome;

/// The exit codes the `orrery` binary may return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Exit {
    /// Success.
    Ok = 0,
    /// The turn completed, but the task failed: a grader or gate said no.
    TaskFailed = 1,
    /// Usage error, including a subcommand that is not implemented yet.
    Usage = 2,
    /// A budget was exceeded.
    Budget = 3,
    /// Denied by policy, with no fallback.
    Denied = 4,
    /// A provider needs a login.
    NeedsLogin = 5,
    /// The kernel or the transport failed.
    Kernel = 6,
}

impl Exit {
    /// Terminate the process with this code.
    pub fn exit(self) -> ! {
        std::process::exit(self as i32)
    }

    /// The code for a turn that ran.
    ///
    /// `tools` is every tool outcome the turn settled, in order. It is needed
    /// because a denial is not a turn outcome: the kernel appends
    /// [`Outcome::Denied`] and asks the model again, so a turn whose work was
    /// refused still reports `Completed`.
    ///
    /// **Decided here:** the last tool call is what decides. A turn that was
    /// denied and then found another way is not a denied turn — it is a turn
    /// with a fallback, and it exits 0. A turn whose *last* call was refused
    /// had no fallback, and that is exit 4. Anything else would make the code
    /// depend on how many times the model tried.
    #[must_use]
    pub fn from_turn(outcome: &TurnOutcome, tools: &[Outcome]) -> Exit {
        match outcome {
            TurnOutcome::Completed { .. } => {
                if matches!(tools.last(), Some(Outcome::Denied { .. })) {
                    Exit::Denied
                } else {
                    Exit::Ok
                }
            }
            TurnOutcome::StoppedByBudget { .. } => Exit::Budget,
            TurnOutcome::NeedsLogin { .. } => Exit::NeedsLogin,
            // Somebody stopped it, so the task did not get done — but nothing
            // broke. `1`, the same code a grader's "no" gets.
            TurnOutcome::Cancelled { .. } => Exit::TaskFailed,
            TurnOutcome::Failed { .. } => Exit::Kernel,
            // `TurnOutcome` is `#[non_exhaustive]`. An ending this build has
            // not been taught about is not a success.
            _ => Exit::Kernel,
        }
    }
}

/// Report that a subcommand is present in the tree but not implemented in this
/// build, naming the plan file that will implement it, and exit [`Exit::Usage`].
///
/// **stdout is data, stderr is narration**: this writes to stderr only, so
/// `--json` stdout stays parseable even for a command that does nothing yet.
pub fn not_implemented(plan: &str) -> ! {
    eprintln!("not implemented in this build — see harness/docs/plans/{plan}");
    Exit::Usage.exit()
}

/// Print a failure on stderr and exit with `code`.
pub fn fail(code: Exit, message: impl std::fmt::Display) -> ! {
    eprintln!("orrery: {message}");
    code.exit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use orrery_proto::{BudgetKind, CancelReason, RuleId, TurnId, Usage};

    fn completed() -> TurnOutcome {
        TurnOutcome::Completed {
            turn: TurnId::new(),
            usage: Usage::default(),
            text: "done".to_owned(),
        }
    }

    #[test]
    fn a_clean_turn_is_zero() {
        assert_eq!(Exit::from_turn(&completed(), &[]), Exit::Ok);
        assert_eq!(
            Exit::from_turn(&completed(), &[Outcome::ok()]),
            Exit::Ok,
            "a tool that worked does not change the code"
        );
    }

    #[test]
    fn a_ceiling_is_three() {
        let outcome = TurnOutcome::StoppedByBudget {
            turn: TurnId::new(),
            kind: BudgetKind::Turns,
            usage: Usage::default(),
        };
        assert_eq!(Exit::from_turn(&outcome, &[]), Exit::Budget);
    }

    #[test]
    fn a_denial_with_no_fallback_is_four() {
        let denied = Outcome::Denied {
            rule: RuleId::new(),
            reason: "no rule allows read(/etc/hosts)".to_owned(),
        };
        assert_eq!(
            Exit::from_turn(&completed(), std::slice::from_ref(&denied)),
            Exit::Denied
        );
        assert_eq!(
            Exit::from_turn(&completed(), &[denied, Outcome::ok()]),
            Exit::Ok,
            "a denial the model worked around is not a denied turn"
        );
    }

    #[test]
    fn needing_a_login_is_five() {
        let outcome = TurnOutcome::NeedsLogin {
            reason: "no api key is configured".to_owned(),
        };
        assert_eq!(Exit::from_turn(&outcome, &[]), Exit::NeedsLogin);
    }

    #[test]
    fn a_cancellation_is_one_and_a_break_is_six() {
        let cancelled = TurnOutcome::Cancelled {
            turn: TurnId::new(),
            reason: CancelReason::User,
            usage: Usage::default(),
        };
        assert_eq!(Exit::from_turn(&cancelled, &[]), Exit::TaskFailed);
        let failed = TurnOutcome::Failed {
            turn: TurnId::new(),
            code: "provider.unavailable".to_owned(),
            message: "the provider gave up".to_owned(),
            usage: Usage::default(),
        };
        assert_eq!(Exit::from_turn(&failed, &[]), Exit::Kernel);
    }
}
