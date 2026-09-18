//! `PermissionHandler` — narrowing only.
//!
//! A handler may narrow: `Allow → Ask | Deny`, `Ask → Deny`. It may never
//! widen. A wider verdict is **dropped and logged**, and it never reaches the
//! managed layer at all. A panic fails **closed**: the call is denied and the
//! session survives.
//!
//! Enforcement is mechanical rather than reviewed. `review` returns a
//! [`Decision`], and [`review_narrowing`] keeps `min(proposed, returned)` under
//! [`crate::Verdict`]'s ordering, with `Deny` lowest. A proptest asserts the
//! result is never greater than `proposed`.
//!
//! # What the handler is actually shown
//!
//! A [`Decision::Allow`] carries a [`crate::CapabilityToken`], which is not
//! `Clone`: handing a handler a live token would be handing it the capability it
//! is there to judge. So the handler is shown a decision of the same verdict
//! whose token was minted against an **inert ledger** — one nothing redeems
//! against. It can read the verdict and the rule, and the capability it might
//! refuse never leaves the engine.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::FutureExt as _;
use orrery_audit::Audit;
use orrery_proto::Subject;

use crate::call::PendingCall;
use crate::engine::Decision;
use crate::error::HandlerError;
use crate::token::{ResolvedScope, TokenLedger, TokenMinter};

/// Somewhere outside the engine that gets a say, downwards only.
#[async_trait]
pub trait PermissionHandler: Send + Sync {
    /// Look at a proposed decision and return one no more permissive.
    ///
    /// # Errors
    ///
    /// A handler that cannot answer returns [`HandlerError`]; the call is then
    /// denied, because a handler that cannot answer is not a handler that said
    /// yes.
    async fn review(
        &self,
        call: &PendingCall,
        subject: &Subject,
        proposed: Decision,
    ) -> Result<Decision, HandlerError>;
}

/// The minter behind the copies handlers are shown. Redeems nowhere.
#[derive(Debug, Clone)]
pub struct InertMinter(TokenMinter);

impl Default for InertMinter {
    fn default() -> Self {
        Self(TokenMinter::new(Arc::new(TokenLedger::new())))
    }
}

impl InertMinter {
    /// A fresh inert minter over its own private ledger.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A decision of the same verdict and rule, carrying nothing redeemable.
    #[must_use]
    pub fn shadow(&self, proposed: &Decision, call: &PendingCall) -> Decision {
        match proposed {
            Decision::Allow { rule, .. } => Decision::Allow {
                token: self.0.mint(
                    call.call,
                    call.aspect,
                    ResolvedScope::new(call.target.clone()),
                    *rule,
                ),
                rule: *rule,
            },
            Decision::Ask {
                prompt,
                rule,
                call: c,
                aspect,
                scope,
                ..
            } => Decision::Ask {
                prompt: prompt.clone(),
                rule: *rule,
                fallback: Box::new(Decision::Deny {
                    rule: *rule,
                    reason: "nobody answered, and an unanswered ask refuses".to_owned(),
                }),
                call: *c,
                aspect: *aspect,
                scope: scope.clone(),
            },
            Decision::Deny { rule, reason } => Decision::Deny {
                rule: *rule,
                reason: reason.clone(),
            },
        }
    }
}

/// Run a handler and keep the narrower of the two answers.
///
/// Fails closed in every direction: an error denies, a panic denies, and a
/// widening answer is discarded in favour of what was proposed.
pub async fn review_narrowing(
    handler: &dyn PermissionHandler,
    call: &PendingCall,
    subject: &Subject,
    proposed: Decision,
    inert: &InertMinter,
    audit: &Audit,
) -> Decision {
    let before = proposed.verdict();
    let rule = proposed.rule();
    let shown = inert.shadow(&proposed, call);

    let reviewed = AssertUnwindSafe(handler.review(call, subject, shown))
        .catch_unwind()
        .await;

    let returned = match reviewed {
        Err(_) => {
            log(audit, call, subject, &HandlerError::Panicked.to_string());
            return Decision::Deny {
                rule,
                reason: HandlerError::Panicked.to_string(),
            };
        }
        Ok(Err(e)) => {
            log(audit, call, subject, &e.to_string());
            return Decision::Deny {
                rule,
                reason: e.to_string(),
            };
        }
        Ok(Ok(d)) => d,
    };

    if returned.verdict() > before {
        log(
            audit,
            call,
            subject,
            &format!(
                "a permission handler tried to widen {before:?} to {:?}; dropped",
                returned.verdict()
            ),
        );
        return proposed;
    }
    // The handler narrowed, or agreed. Either way `min` is computed, not
    // trusted: this is the line the proptest is about.
    proposed.min(returned)
}

fn log(audit: &Audit, call: &PendingCall, subject: &Subject, message: &str) {
    tracing::warn!(target: "orrery.audit.policy", %subject, call = %call.match_text(), message);
    audit.append(orrery_audit::AuditEvent::CapabilityDecision {
        subject: subject.clone(),
        request: call.match_text(),
        verdict: orrery_audit::Verdict::Deny,
        rule: None,
        rule_text: None,
        layer: None,
        reason: Some(message.to_owned()),
    });
}
