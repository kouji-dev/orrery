//! Retry is the kernel's, and it is charged to the turn.
//!
//! Provider code never sleeps: it classifies with
//! [`ProviderError::is_retryable`](orrery_provider::ProviderError::is_retryable)
//! and the kernel decides whether to spend more budget. That division is what
//! makes a budget mean anything — three providers each backing off privately
//! would produce a turn that took a minute and reported nothing about why.
//!
//! So the backoff happens here, the time it takes counts against the turn's wall
//! clock (it is measured from one `Instant`, and sleeping does not stop it), and
//! every attempt is audited.

use std::time::Duration;

use orrery_provider::ProviderError;

/// How many times, and how long between.
///
/// TODO(plan-10): these come from profile config, per provider and per profile
/// — see this plan's open question 1. Hardcoded until then, and hardcoded
/// *here* rather than in each provider, so that changing them is one edit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    /// How many attempts in total, the first one included. One means no retry.
    pub max_attempts: u32,
    /// The first backoff.
    pub base_ms: u64,
    /// The longest backoff, however many doublings it takes.
    pub max_ms: u64,
    /// Whether to spread the backoff, so a hundred sessions rate-limited at
    /// once do not come back in lockstep.
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_ms: 250,
            max_ms: 8_000,
            jitter: true,
        }
    }
}

impl RetryPolicy {
    /// Never retry. For a test that wants the failure now.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            max_attempts: 1,
            base_ms: 0,
            max_ms: 0,
            jitter: false,
        }
    }

    /// Whether another attempt is allowed after this many have been made.
    #[must_use]
    pub const fn may_retry(&self, attempts_made: u32) -> bool {
        attempts_made < self.max_attempts
    }

    /// How long to wait before attempt number `attempt` (one-based, so the
    /// first retry is attempt 2).
    ///
    /// A provider that said `Retry-After` is obeyed rather than doubled past:
    /// it knows when it will serve us and we do not.
    #[must_use]
    pub fn backoff(&self, attempt: u32, error: &ProviderError) -> Duration {
        if let ProviderError::RateLimited {
            retry_after_ms: Some(ms),
        } = error
        {
            return Duration::from_millis((*ms).min(self.max_ms.max(*ms)));
        }
        let doublings = attempt.saturating_sub(1).min(16);
        let plain = self
            .base_ms
            .saturating_mul(1u64 << doublings)
            .min(self.max_ms);
        Duration::from_millis(if self.jitter { jitter(plain) } else { plain })
    }
}

/// Full jitter, from the clock rather than from a random-number dependency.
///
/// The requirement is "two sessions do not come back in the same millisecond",
/// not unpredictability, so the low bits of the wall clock are enough and cost
/// nothing on the dependency graph.
fn jitter(ms: u64) -> u64 {
    if ms == 0 {
        return 0;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos() as u64);
    // Between half and all of it: still backing off, never longer than asked.
    let half = ms / 2;
    half + (nanos % (ms - half).max(1))
}

#[cfg(test)]
mod tests {
    use super::RetryPolicy;
    use orrery_provider::ProviderError;

    #[test]
    fn backoff_doubles_and_is_capped() {
        let policy = RetryPolicy {
            max_attempts: 6,
            base_ms: 100,
            max_ms: 400,
            jitter: false,
        };
        let e = ProviderError::ServerError { status: 503 };
        assert_eq!(policy.backoff(1, &e).as_millis(), 100);
        assert_eq!(policy.backoff(2, &e).as_millis(), 200);
        assert_eq!(policy.backoff(3, &e).as_millis(), 400);
        assert_eq!(policy.backoff(4, &e).as_millis(), 400, "capped");
    }

    #[test]
    fn retry_after_is_obeyed() {
        let policy = RetryPolicy::default();
        let waited = policy.backoff(
            1,
            &ProviderError::RateLimited {
                retry_after_ms: Some(1_200),
            },
        );
        assert_eq!(waited.as_millis(), 1_200);
    }

    #[test]
    fn jitter_never_exceeds_the_plain_backoff() {
        let policy = RetryPolicy {
            max_attempts: 3,
            base_ms: 1_000,
            max_ms: 1_000,
            jitter: true,
        };
        let e = ProviderError::Timeout { elapsed_ms: 1 };
        for _ in 0..50 {
            let ms = policy.backoff(1, &e).as_millis() as u64;
            assert!((500..=1_000).contains(&ms), "jittered to {ms}ms");
        }
    }
}
