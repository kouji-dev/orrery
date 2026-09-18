//! What an interceptor decided.

use crate::frame::Outcome;

/// The answer an interceptor gives about one phase.
///
/// Generic over the phase's payload so the type system enforces what a comment
/// otherwise would: an interceptor watching a tool call can rewrite a tool
/// input and **cannot** hand back a model request, because those are different
/// `P`s. The `Phase` trait that ties a phase to its `P` is the kernel's (plan
/// 05); the verdict lives here so that an extension API can name it without
/// depending on the kernel.
///
/// No serde: a verdict is returned across a function call, never across the
/// wire. Only `Debug`, for test output and tracing.
#[non_exhaustive]
#[derive(Debug)]
pub enum Verdict<P> {
    /// Nothing to say; carry on to the next interceptor, then to the kernel.
    Continue,
    /// Carry on, but with this instead.
    Rewrite(P),
    /// Do not carry on. Becomes an [`Outcome::Denied`] naming the rule.
    Deny {
        /// Why, in words a person can act on.
        reason: String,
    },
    /// Do not carry on; the answer is already here.
    ///
    /// The cache case: an interceptor holding the result answers from what it
    /// has, and the call never reaches the tool.
    Handled {
        /// What to report as having happened.
        result: Outcome,
    },
}

impl<P> Verdict<P> {
    /// Whether this verdict stops the phase from running.
    #[must_use]
    pub fn is_final(&self) -> bool {
        matches!(self, Verdict::Deny { .. } | Verdict::Handled { .. })
    }
}
