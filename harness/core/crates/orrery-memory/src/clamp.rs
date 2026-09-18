//! The token clamp is ours; the store is theirs.
//!
//! `recall` runs during `context.build` under a **token allowance set by the
//! profile**, and what comes back is clipped to it. Memory shares the window
//! with history, and a chatty provider must not quietly evict the transcript —
//! so the split happens before the provider is asked, and the clip happens
//! after it answers. Neither is negotiable with the provider.
//!
//! An entry that does not fit is dropped **whole**. Half a recalled note is
//! worse than none: the model cannot tell it was truncated.

use orrery_proto::TokenBudget;

use crate::provider::MemEntry;

/// How to price text in tokens.
///
/// A trait rather than a constant, because a provider-accurate tokeniser is a
/// dependency this crate does not have and will not take. The default is the
/// same four-chars-to-a-token estimate `orrery-session` materialises with, so
/// the two halves of the window are measured the same way.
pub trait TokenCount: Send + Sync {
    /// What this text costs.
    fn count_text(&self, text: &str) -> u64;

    /// What an entry costs, key included: the key is rendered into the context
    /// too.
    fn count_entry(&self, entry: &MemEntry) -> u64 {
        self.count_text(&entry.key)
            .saturating_add(self.count_text(&entry.text))
    }
}

/// Four characters to a token.
#[derive(Copy, Clone, Debug, Default)]
pub struct CharsOverFour;

impl TokenCount for CharsOverFour {
    fn count_text(&self, text: &str) -> u64 {
        (text.chars().count() / 4) as u64
    }
}

/// What survived the clamp, and what did not.
#[derive(Clone, Debug, PartialEq)]
pub struct Clamped {
    /// What fits, in the order the provider returned it.
    pub entries: Vec<MemEntry>,
    /// How many entries were dropped. **Recorded, never silent.**
    pub dropped: usize,
    /// What the survivors cost.
    pub used_tokens: u64,
    /// What they were allowed to cost.
    pub allowance: u64,
}

/// Fit what a provider returned into what the profile allowed.
///
/// Greedy in the provider's own order: the provider ranked them, and re-ranking
/// its answer by length would be the kernel second-guessing the retrieval
/// strategy it deliberately does not own.
#[must_use]
pub fn clamp(entries: Vec<MemEntry>, budget: TokenBudget, counter: &dyn TokenCount) -> Clamped {
    let allowance = budget.available();
    let mut used = 0u64;
    let mut kept = Vec::new();
    let mut dropped = 0usize;
    for entry in entries {
        let cost = counter.count_entry(&entry);
        if used.saturating_add(cost) > allowance {
            dropped += 1;
            continue;
        }
        used += cost;
        kept.push(entry);
    }
    Clamped {
        entries: kept,
        dropped,
        used_tokens: used,
        allowance,
    }
}

/// The window, split between memory and everything else.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WindowSplit {
    /// What memory may fill.
    pub memory: TokenBudget,
    /// What is left for history and the current input.
    pub history: TokenBudget,
}

/// Give memory a declared share of the window and history the rest.
///
/// The share is taken off the **available** half, so the model's own output
/// reserve is never spent on recalled notes. `share` is clamped to `0.0..=1.0`;
/// the two halves always add back up to `window.available()`, so no token is
/// invented or lost in the split.
#[must_use]
pub fn split_window(window: TokenBudget, share: f64) -> WindowSplit {
    let available = window.available();
    let share = share.clamp(0.0, 1.0);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    #[allow(clippy::cast_precision_loss)]
    let memory = ((available as f64) * share).round() as u64;
    let memory = memory.min(available);
    WindowSplit {
        memory: TokenBudget {
            max: memory,
            reserve: 0,
        },
        history: TokenBudget {
            max: available - memory,
            reserve: 0,
        },
    }
}

/// The default share of the window memory gets when a profile does not say.
///
/// A tenth: enough to be useful, small enough that a chatty provider is an
/// annoyance rather than an outage.
pub const DEFAULT_MEMORY_SHARE: f64 = 0.1;
