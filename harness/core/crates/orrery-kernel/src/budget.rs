//! The ceilings the kernel enforces rather than trusts.
//!
//! Four of them — turns, tokens, wall clock, money — checked **before each
//! pass** and **before each tool dispatch**, which are the two places where a
//! turn is about to become more expensive than it already is.
//!
//! # Running out is a value
//!
//! [`check`](TurnBudget::check) answers `Option<BudgetKind>` and the loop turns
//! that into [`TurnOutcome::StoppedByBudget`](crate::TurnOutcome). There is no
//! error path, and `tests/budget.rs` asserts the absence of a budget variant on
//! [`KernelError`](crate::KernelError): a ceiling is the system working, and a
//! caller should not have to catch it.
//!
//! # Zero means unmetered
//!
//! Every field of [`Budget`] is a plain number, so `Budget::default()` is all
//! zeros. Reading zero as "stop immediately" would make the default budget stop
//! every turn before it began, so zero is **no limit** — spelled out here
//! because it is the kind of convention that is otherwise discovered.

use std::time::Instant;

use orrery_proto::{Budget, BudgetKind, Usage};

/// What a model's tokens cost.
///
/// A `[prices.<model>]` table in configuration fills this;
/// `orrery_harness::price_table` is where the keys are read and
/// `KernelConfig::prices` is where it arrives. The kernel itself still knows
/// nothing about layers or profiles: it is handed a table, and prices with it.
///
/// A model **nobody priced** stays unpriced, and [`PriceTable::empty`] leaves
/// `micro_usd` as `None`, which makes a `maxUsd` ceiling **inert** rather than
/// wrong — a budget that silently priced everything at zero would report "under
/// budget" for a session that spent a hundred dollars. That is a documented
/// state, not a gap: `orrery-cli`'s `tests/budget.rs` asserts both halves
/// through the binary.
#[derive(Clone, Debug, Default)]
pub struct PriceTable {
    entries: Vec<(String, u64, u64)>,
}

impl PriceTable {
    /// A table that prices nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Price one model, in micro-USD per million tokens in and out.
    #[must_use]
    pub fn with(mut self, model: impl Into<String>, input: u64, output: u64) -> Self {
        self.entries.push((model.into(), input, output));
        self
    }

    /// What this usage cost, when the table knows the model.
    #[must_use]
    pub fn micro_usd(&self, model: &str, usage: &Usage) -> Option<u64> {
        let (_, per_in, per_out) = self.entries.iter().find(|(m, _, _)| m == model)?;
        let cost = |tokens: u64, per_million: u64| {
            tokens
                .saturating_mul(per_million)
                .checked_div(1_000_000)
                .unwrap_or(0)
        };
        Some(cost(usage.input_tokens, *per_in).saturating_add(cost(usage.output_tokens, *per_out)))
    }

    /// Whether anything is priced at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// What one turn has spent, and what it is allowed to.
#[derive(Debug)]
pub struct TurnBudget {
    spent: Usage,
    turns: u32,
    started: Instant,
    limit: Budget,
    prices: PriceTable,
    model: String,
}

impl TurnBudget {
    /// A budget that has spent nothing.
    #[must_use]
    pub fn new(limit: Budget) -> Self {
        Self {
            spent: Usage::default(),
            turns: 0,
            started: Instant::now(),
            limit,
            prices: PriceTable::empty(),
            model: String::new(),
        }
    }

    /// Price this turn's usage with a table.
    #[must_use]
    pub fn priced(mut self, model: impl Into<String>, prices: PriceTable) -> Self {
        self.model = model.into();
        self.prices = prices;
        self
    }

    /// Which ceiling has been reached, if any.
    ///
    /// Called before each pass and before each tool dispatch.
    #[must_use]
    pub fn check(&self) -> Option<BudgetKind> {
        if self.limit.max_turns > 0 && self.turns >= self.limit.max_turns {
            return Some(BudgetKind::Turns);
        }
        if self.limit.max_tokens > 0 && self.spent.total_tokens() >= self.limit.max_tokens {
            return Some(BudgetKind::Tokens);
        }
        if self.limit.wall_clock_ms > 0 && self.elapsed_ms() >= self.limit.wall_clock_ms {
            return Some(BudgetKind::WallClock);
        }
        match (self.limit.max_micro_usd, self.spent.micro_usd) {
            (Some(cap), Some(spent)) if cap > 0 && spent >= cap => Some(BudgetKind::Usd),
            // No price table, no money ceiling. Inert, not zero.
            _ => None,
        }
    }

    /// Add what a pass cost, pricing it if the table can.
    pub fn charge(&mut self, usage: &Usage) {
        let mut usage = *usage;
        if usage.micro_usd.is_none() {
            usage.micro_usd = self.prices.micro_usd(&self.model, &usage);
        }
        self.spent += usage;
    }

    /// Record that another pass has been taken.
    pub fn begin_pass(&mut self) {
        self.turns = self.turns.saturating_add(1);
    }

    /// How many passes have begun.
    #[must_use]
    pub fn turns(&self) -> u32 {
        self.turns
    }

    /// What has been spent.
    #[must_use]
    pub fn spent(&self) -> &Usage {
        &self.spent
    }

    /// How long the turn has been running.
    #[must_use]
    pub fn elapsed_ms(&self) -> u64 {
        self.started
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }

    /// What is left on the wall clock, for a timeout on a pass.
    #[must_use]
    pub fn remaining_ms(&self) -> Option<u64> {
        if self.limit.wall_clock_ms == 0 {
            return None;
        }
        Some(self.limit.wall_clock_ms.saturating_sub(self.elapsed_ms()))
    }

    /// The ceilings themselves.
    #[must_use]
    pub fn limit(&self) -> &Budget {
        &self.limit
    }
}

#[cfg(test)]
mod tests {
    use super::{PriceTable, TurnBudget};
    use orrery_proto::{Budget, BudgetKind, Usage};

    #[test]
    fn zero_is_unmetered() {
        let budget = TurnBudget::new(Budget::default());
        assert_eq!(budget.check(), None, "a default budget stops nothing");
    }

    #[test]
    fn turns_stop_at_the_ceiling() {
        let mut budget = TurnBudget::new(Budget {
            max_turns: 2,
            ..Budget::default()
        });
        budget.begin_pass();
        assert_eq!(budget.check(), None);
        budget.begin_pass();
        assert_eq!(budget.check(), Some(BudgetKind::Turns));
    }

    #[test]
    fn money_is_inert_without_a_price_table() {
        let mut budget = TurnBudget::new(Budget {
            max_micro_usd: Some(1),
            ..Budget::default()
        });
        budget.charge(&Usage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..Usage::default()
        });
        assert_eq!(
            budget.check(),
            None,
            "an unpriced model must not trip a money ceiling"
        );

        let mut priced = TurnBudget::new(Budget {
            max_micro_usd: Some(1),
            ..Budget::default()
        })
        .priced("m", PriceTable::empty().with("m", 3_000_000, 15_000_000));
        priced.charge(&Usage {
            input_tokens: 1_000_000,
            ..Usage::default()
        });
        assert_eq!(priced.check(), Some(BudgetKind::Usd));
    }
}
