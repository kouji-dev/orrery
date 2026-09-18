//! Budgets and usage.
//!
//! Money is **micro-USD as a `u64`**, never a float: a budget that has to be
//! compared for exhaustion, summed across passes and written to an audit log
//! cannot be allowed to drift by a rounding error, and `1e-6` USD is finer than
//! any provider prices at.

use serde::{Deserialize, Serialize};

/// What a turn is allowed to spend.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct Budget {
    /// How many kernel turns.
    pub max_turns: u32,
    /// How many tokens, input and output together.
    pub max_tokens: u64,
    /// How long, in milliseconds of wall clock.
    pub wall_clock_ms: u64,
    /// How much money, in micro-USD. `None` means unmetered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_micro_usd: Option<u64>,
}

/// Which budget ran out.
///
/// Carried by a stop reason so that "we stopped" always says *which* limit bit.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BudgetKind {
    /// [`Budget::max_turns`].
    Turns,
    /// [`Budget::max_tokens`].
    Tokens,
    /// [`Budget::wall_clock_ms`].
    WallClock,
    /// [`Budget::max_micro_usd`].
    Usd,
}

/// What was actually spent.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct Usage {
    /// Tokens sent.
    pub input_tokens: u64,
    /// Tokens received.
    pub output_tokens: u64,
    /// Tokens served from a provider-side prompt cache.
    pub cache_hits: u64,
    /// Cost in micro-USD, when the provider reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub micro_usd: Option<u64>,
}

impl Usage {
    /// Input plus output. Cache hits are already counted in `input_tokens`.
    #[must_use]
    pub const fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

impl std::ops::AddAssign for Usage {
    /// Accumulate across passes.
    ///
    /// **Saturating**, not wrapping: a runaway session should pin a counter at
    /// the maximum and trip every budget, not wrap to zero and look free.
    fn add_assign(&mut self, rhs: Self) {
        self.input_tokens = self.input_tokens.saturating_add(rhs.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(rhs.output_tokens);
        self.cache_hits = self.cache_hits.saturating_add(rhs.cache_hits);
        self.micro_usd = match (self.micro_usd, rhs.micro_usd) {
            (None, None) => None,
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (Some(a), Some(b)) => Some(a.saturating_add(b)),
        };
    }
}

impl std::ops::Add for Usage {
    type Output = Usage;

    fn add(mut self, rhs: Self) -> Self {
        self += rhs;
        self
    }
}

impl std::iter::Sum for Usage {
    fn sum<I: Iterator<Item = Usage>>(iter: I) -> Usage {
        iter.fold(Usage::default(), |acc, u| acc + u)
    }
}

/// A token ceiling with a slice held back.
///
/// Used by the recall clamp and by `materialise`: `max` is the context window
/// the provider will accept, `reserve` is what must stay free for the model's
/// own output.
#[derive(
    Copy, Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct TokenBudget {
    /// The ceiling.
    pub max: u64,
    /// What to keep free under it.
    pub reserve: u64,
}

impl TokenBudget {
    /// What may actually be filled. Saturates at zero rather than underflowing
    /// when the reserve is larger than the ceiling.
    #[must_use]
    pub const fn available(&self) -> u64 {
        self.max.saturating_sub(self.reserve)
    }
}
