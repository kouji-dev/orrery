//! The only door a cost number may come through.
//!
//! # Why this type has the shape it has
//!
//! [`BoundaryMeter`] has **no method that takes a `Usage`**. The only way to
//! put a number into it is [`BoundaryMeter::observe`], which takes a
//! `&ModelEvent` — the value the provider itself emitted — and ignores every
//! variant but [`ModelEvent::Usage`]. So "the cost is read at the provider
//! boundary, never estimated afterwards" is not a convention anybody has to
//! remember: there is no other API to reach for. A runner that wanted to
//! estimate would have to fabricate a `ModelEvent::Usage`, which is a thing a
//! reviewer sees.
//!
//! # Proving the negative
//!
//! Absence of an estimation path is hard to assert directly, so
//! [`EstimatorProbe`] wraps the provider's own [`TokenCounter`] and counts
//! every question anybody asks it. `run::cost_comes_from_telemetry` asserts the
//! count is zero for a whole run: nothing consulted an estimator, so nothing
//! could have derived the cost from one.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use orrery_proto::{Message, Role, Usage};
use orrery_provider::{ModelEvent, TokenCounter};

use crate::report::{ByRole, CostProvenance};

/// Sums what the provider said, per role, as it says it.
#[derive(Debug, Default)]
pub struct BoundaryMeter {
    inner: Mutex<Meter>,
    events: AtomicU64,
}

#[derive(Debug, Default)]
struct Meter {
    total: Usage,
    by_role: ByRole,
    open_pass: Option<Role>,
}

impl BoundaryMeter {
    /// A meter that has seen nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a pass for a role.
    ///
    /// Attribution is by pass, so the meter is told which part of the loop is
    /// about to run rather than inferring it from the model id. That is what
    /// makes the numbers separate even when every role is bound to the same
    /// model.
    pub fn begin_pass(&self, role: Role) {
        let mut m = self.inner.lock().expect("meter");
        m.open_pass = Some(role);
        m.by_role.touch(role);
    }

    /// Close the open pass, counting it even if the provider reported no usage.
    pub fn end_pass(&self) {
        let mut m = self.inner.lock().expect("meter");
        if let Some(role) = m.open_pass.take() {
            m.by_role.count_pass(role);
        }
    }

    /// Look at one event from the provider stream.
    ///
    /// Everything but [`ModelEvent::Usage`] is ignored. This is the only way a
    /// number enters the meter.
    pub fn observe(&self, event: &ModelEvent) {
        self.events.fetch_add(1, Ordering::Relaxed);
        let ModelEvent::Usage { usage } = event else {
            return;
        };
        let mut m = self.inner.lock().expect("meter");
        m.total += *usage;
        let role = m.open_pass;
        if let Some(role) = role {
            m.by_role.add(role, *usage);
        }
    }

    /// What the whole run cost.
    #[must_use]
    pub fn total(&self) -> Usage {
        self.inner.lock().expect("meter").total
    }

    /// Where it went.
    #[must_use]
    pub fn by_role(&self) -> ByRole {
        self.inner.lock().expect("meter").by_role.clone()
    }

    /// How many provider events passed through. Instrumentation, not a cost.
    #[must_use]
    pub fn events_seen(&self) -> u64 {
        self.events.load(Ordering::Relaxed)
    }

    /// Always [`CostProvenance::MeasuredAtProviderBoundary`]: it is the only
    /// thing a meter can honestly claim.
    #[must_use]
    pub fn provenance(&self) -> CostProvenance {
        CostProvenance::MeasuredAtProviderBoundary
    }
}

/// A provider's counter, as an owned value.
///
/// [`Provider::counter`](orrery_provider::Provider::counter) hands back an
/// `Arc<dyn TokenCounter>`, which is not itself a `TokenCounter`. This is the
/// one-line bridge, so that the probe can stand in front of it.
#[derive(Clone)]
pub struct SharedCounter(std::sync::Arc<dyn TokenCounter>);

impl SharedCounter {
    /// Wrap a provider's counter.
    #[must_use]
    pub fn new(inner: std::sync::Arc<dyn TokenCounter>) -> Self {
        Self(inner)
    }
}

impl std::fmt::Debug for SharedCounter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SharedCounter")
    }
}

impl TokenCounter for SharedCounter {
    fn count_messages(&self, messages: &[Message]) -> u64 {
        self.0.count_messages(messages)
    }

    fn count_text(&self, text: &str) -> u64 {
        self.0.count_text(text)
    }

    fn is_exact(&self) -> bool {
        self.0.is_exact()
    }
}

/// A [`TokenCounter`] that counts how often it is asked.
///
/// Wrap the provider's own counter in one of these for a run and assert
/// [`EstimatorProbe::questions`] is zero afterwards: an estimate that was never
/// asked for cannot have become the reported cost.
#[derive(Debug)]
pub struct EstimatorProbe<C> {
    inner: C,
    questions: AtomicU64,
}

impl<C> EstimatorProbe<C> {
    /// Wrap a counter.
    pub const fn new(inner: C) -> Self {
        Self {
            inner,
            questions: AtomicU64::new(0),
        }
    }

    /// How many times anybody asked this counter for an estimate.
    pub fn questions(&self) -> u64 {
        self.questions.load(Ordering::Relaxed)
    }
}

impl<C: TokenCounter> TokenCounter for EstimatorProbe<C> {
    fn count_messages(&self, messages: &[Message]) -> u64 {
        self.questions.fetch_add(1, Ordering::Relaxed);
        self.inner.count_messages(messages)
    }

    fn count_text(&self, text: &str) -> u64 {
        self.questions.fetch_add(1, Ordering::Relaxed);
        self.inner.count_text(text)
    }

    fn is_exact(&self) -> bool {
        self.inner.is_exact()
    }
}
