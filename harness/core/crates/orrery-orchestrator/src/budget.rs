//! The ceiling applies to the **whole workflow**, not to each step.
//!
//! A per-step budget multiplied by the number of steps is not a budget: it is a
//! number that grows every time somebody adds a step. So one
//! [`WorkflowBudget`] is threaded through the run and every step charges
//! against it, which is `workflow::budget_is_whole_workflow`.

use orrery_proto::{Budget, BudgetKind, Usage};

/// What is left of a workflow's ceiling.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkflowBudget {
    limit: Budget,
    spent: Usage,
    steps: u32,
}

/// A ceiling that has been reached, and which one.
#[derive(Copy, Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("the workflow budget is spent: {kind:?}")]
pub struct Exceeded {
    /// Which ceiling bit.
    pub kind: BudgetKind,
}

impl WorkflowBudget {
    /// A fresh budget.
    #[must_use]
    pub fn new(limit: Budget) -> Self {
        Self {
            limit,
            spent: Usage::default(),
            steps: 0,
        }
    }

    /// The ceiling it was built with.
    #[must_use]
    pub const fn limit(&self) -> Budget {
        self.limit
    }

    /// What has been spent so far, across every step.
    #[must_use]
    pub const fn spent(&self) -> Usage {
        self.spent
    }

    /// How many steps have been charged.
    #[must_use]
    pub const fn steps(&self) -> u32 {
        self.steps
    }

    /// Tokens still unspent.
    #[must_use]
    pub const fn remaining_tokens(&self) -> u64 {
        self.limit
            .max_tokens
            .saturating_sub(self.spent.total_tokens())
    }

    /// Whether there is anything left to start another step with.
    ///
    /// Checked **before** a step runs, so a workflow stops rather than
    /// overspending and reporting it afterwards.
    ///
    /// # Errors
    ///
    /// [`Exceeded`], naming the ceiling that bit.
    pub fn check(&self) -> Result<(), Exceeded> {
        if self.limit.max_turns > 0 && self.steps >= self.limit.max_turns {
            return Err(Exceeded {
                kind: BudgetKind::Turns,
            });
        }
        if self.limit.max_tokens > 0 && self.spent.total_tokens() >= self.limit.max_tokens {
            return Err(Exceeded {
                kind: BudgetKind::Tokens,
            });
        }
        if let (Some(cap), Some(spent)) = (self.limit.max_micro_usd, self.spent.micro_usd)
            && spent >= cap
        {
            return Err(Exceeded {
                kind: BudgetKind::Usd,
            });
        }
        Ok(())
    }

    /// Charge one step's usage against the whole-workflow ceiling.
    ///
    /// # Errors
    ///
    /// [`Exceeded`] when this step is what took it over. The usage is recorded
    /// either way: what was spent was spent.
    pub fn charge(&mut self, usage: Usage) -> Result<(), Exceeded> {
        self.spent += usage;
        self.steps = self.steps.saturating_add(1);
        self.check()
    }

    /// What one child may spend, when `n` of them are about to start.
    ///
    /// A slice of what is **left**, so N children can never together exceed the
    /// parent — which is the same arithmetic `orrery-router` slices a fan-out
    /// with.
    #[must_use]
    pub fn slice(&self, n: u32) -> Budget {
        let n = u64::from(n.max(1));
        Budget {
            max_turns: self.limit.max_turns,
            max_tokens: self.remaining_tokens() / n,
            wall_clock_ms: self.limit.wall_clock_ms,
            max_micro_usd: self
                .limit
                .max_micro_usd
                .map(|usd| usd.saturating_sub(self.spent.micro_usd.unwrap_or(0)) / n),
        }
    }
}
