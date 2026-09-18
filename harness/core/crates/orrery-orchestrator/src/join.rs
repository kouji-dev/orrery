//! How parallel branches are joined.
//!
//! # Quorum cancels the rest (open question 3, decided)
//!
//! `Quorum(n)` proceeds as soon as `n` branches have answered and **cancels**
//! the others. Waiting for all of them and then using `n` would pay for every
//! branch and use some of them, which is the expensive reading of a feature
//! whose whole point is to stop early. Cancellation is what
//! [`Join::wanted`] means and what `workflow::parallel_join_all_first_quorum`
//! asserts.
//!
//! `First` is `Quorum(1)`, spelled separately because it is what people write.

use serde::{Deserialize, Serialize};

/// How a [`Parallel`](crate::Step::Parallel) step joins.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Join {
    /// Wait for every branch.
    All,
    /// Take the first answer and cancel the rest.
    First,
    /// Take the first `n` answers and cancel the rest.
    Quorum(u32),
}

impl Join {
    /// How many answers this join needs out of `total` branches.
    ///
    /// Clamped to `total`: a quorum larger than the number of branches would
    /// otherwise wait for an answer that cannot arrive.
    #[must_use]
    pub fn wanted(self, total: usize) -> usize {
        let total = total.max(1);
        match self {
            Join::All => total,
            Join::First => 1.min(total),
            Join::Quorum(n) => (n as usize).clamp(1, total),
        }
    }

    /// Whether this join leaves branches to cancel.
    #[must_use]
    pub fn cancels(self, total: usize) -> bool {
        self.wanted(total) < total
    }

    /// The word it is written with.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Join::All => "all",
            Join::First => "first",
            Join::Quorum(_) => "quorum",
        }
    }
}

/// What a join came to.
#[derive(Clone, Debug, PartialEq)]
pub struct Joined {
    /// The values that arrived, in branch declaration order.
    pub values: Vec<(String, serde_json::Value)>,
    /// How many branches were cancelled without finishing.
    pub cancelled: usize,
}
