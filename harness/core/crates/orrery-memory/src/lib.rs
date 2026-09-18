//! The MemoryProvider trait, scope lifetimes, the token clamp and the visibility rule. No provider.
//!
//! Memory is the one component we deliberately do not implement. **The kernel
//! owns scoping, lifetime and visibility; an extension owns the store, the
//! content and the retrieval strategy.** Reads enter at `context.build`, writes
//! only from lifecycle handlers, and a sub-agent's notes die with its branch
//! without anyone writing cleanup code.
//!
//! Four properties, each held by a type rather than by a convention:
//!
//! - **Writes cannot happen inside the loop.** [`MemoryProvider::write`] takes a
//!   [`LifecycleWitness`], which only a [`LifecycleCtx`] can produce. See
//!   [`witness`].
//! - **A scope's lifetime is a kernel object's lifetime.** Dropping a
//!   [`ScopeGuard`] is what discarding a branch, a turn or a session *is*. See
//!   [`scope`].
//! - **Visibility is the kernel's, not the store's.** A provider is handed a
//!   pre-filtered scope list and never sees a write it should have refused. See
//!   [`visibility`].
//! - **The token clamp is ours; the store is theirs.** A chatty provider is
//!   clipped and the clip is recorded. See [`clamp`].
//!
//! What memory injected is recorded in the turn as **resolved content**
//! ([`orrery_session::TurnKind::Recalled`]), never as a pointer: the store moves
//! on, and the tree still has to show what the model saw.
//!
//! Implementation plan: `harness/docs/plans/12-memory.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod clamp;
pub mod conformance;
pub mod error;
pub mod ledger;
pub mod perm;
pub mod provider;
pub mod scope;
pub mod testing;
pub mod visibility;
pub mod witness;

pub use clamp::{
    CharsOverFour, Clamped, DEFAULT_MEMORY_SHARE, TokenCount, WindowSplit, clamp, split_window,
};
pub use error::MemError;
pub use ledger::MemEvent;
pub use perm::{AllowAll, MemPermissions, request_text};
pub use provider::{MemEntry, MemSelector, MemoryProvider, RecallQuery};
pub use scope::{Lifetimes, MemScope, ScopeGuard, ScopeKind};
pub use visibility::Actor;
pub use witness::{InterceptCtx, LifecycleCtx, LifecyclePoint, LifecycleWitness};

use std::sync::Arc;

use orrery_proto::{Aspect, Message, MessageRole, TokenBudget};
use orrery_session::{RecalledEntry, TurnKind};
use parking_lot::Mutex;

/// What one provider contributed to one context.
///
/// One per provider, because [`TurnKind::Recalled`] names a provider: two
/// providers produce two rows, and a replay says which store said what.
#[derive(Clone, Debug, PartialEq)]
pub struct Recall {
    /// Which provider answered.
    pub provider: String,
    /// What it returned, after the clamp, verbatim.
    pub entries: Vec<RecalledEntry>,
    /// How many entries the clamp dropped.
    pub dropped: usize,
    /// What the survivors cost.
    pub used_tokens: u64,
    /// What they were allowed to cost.
    pub allowance: u64,
}

impl Recall {
    /// The row the turn tree stores: resolved content, never a pointer.
    #[must_use]
    pub fn to_turn_kind(&self) -> TurnKind {
        TurnKind::Recalled {
            provider: self.provider.clone(),
            entries: self.entries.clone(),
        }
    }

    /// Whether anything survived.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Render recalled entries as the one message the volatile suffix starts with.
///
/// One message rather than one each, so that eliding memory is one decision.
/// Byte-identical to `orrery_session`'s own rendering of a
/// [`TurnKind::Recalled`] row, which is what makes a replayed session reproduce
/// the context it originally had.
#[must_use]
pub fn recalled_message(provider: &str, entries: &[RecalledEntry]) -> Option<Message> {
    if entries.is_empty() {
        return None;
    }
    Some(Message::text(
        MessageRole::User,
        entries
            .iter()
            .map(|e| format!("[{provider}:{}] {}", e.key, e.text))
            .collect::<Vec<_>>()
            .join("\n"),
    ))
}

/// The half of memory the kernel owns.
///
/// Holds the providers, the scope lifetimes, the permission check and the
/// allowance. Everything a provider is not allowed to decide is decided here,
/// before the provider is reached.
pub struct MemoryKernel {
    providers: Vec<Arc<dyn MemoryProvider>>,
    lifetimes: Arc<Lifetimes>,
    permissions: Arc<dyn MemPermissions>,
    allowance: TokenBudget,
    counter: Arc<dyn TokenCount>,
    ledger: Mutex<Vec<MemEvent>>,
}

impl std::fmt::Debug for MemoryKernel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryKernel")
            .field(
                "providers",
                &self.providers.iter().map(|p| p.id()).collect::<Vec<_>>(),
            )
            .field("allowance", &self.allowance)
            .finish_non_exhaustive()
    }
}

impl Default for MemoryKernel {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryKernel {
    /// A kernel with no providers: memory is simply absent, which is a
    /// supported configuration and the default one.
    #[must_use]
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            lifetimes: Lifetimes::new(),
            permissions: Arc::new(AllowAll),
            allowance: TokenBudget { max: 0, reserve: 0 },
            counter: Arc::new(CharsOverFour),
            ledger: Mutex::new(Vec::new()),
        }
    }

    /// Add a provider. Zero or many are allowed.
    #[must_use]
    pub fn with_provider(mut self, provider: Arc<dyn MemoryProvider>) -> Self {
        self.providers.push(provider);
        self
    }

    /// Wire in the permission check. Without one, nothing is refused on
    /// permission grounds — the visibility rule still applies.
    #[must_use]
    pub fn with_permissions(mut self, permissions: Arc<dyn MemPermissions>) -> Self {
        self.permissions = permissions;
        self
    }

    /// The profile's token allowance for memory, across every provider.
    #[must_use]
    pub const fn with_allowance(mut self, allowance: TokenBudget) -> Self {
        self.allowance = allowance;
        self
    }

    /// Take the memory half of a context window, leaving history its own.
    #[must_use]
    pub fn with_window(self, window: TokenBudget, share: f64) -> Self {
        let split = split_window(window, share);
        self.with_allowance(split.memory)
    }

    /// Price text the way the provider's model does.
    #[must_use]
    pub fn with_counter(mut self, counter: Arc<dyn TokenCount>) -> Self {
        self.counter = counter;
        self
    }

    /// The scope lifetimes. Entering a scope here is how a branch, a turn or a
    /// session gets a memory lifetime; dropping the guard is the cleanup.
    #[must_use]
    pub const fn lifetimes(&self) -> &Arc<Lifetimes> {
        &self.lifetimes
    }

    /// Everything memory has done, for the host to forward to the audit stream.
    #[must_use]
    pub fn ledger(&self) -> Vec<MemEvent> {
        self.ledger.lock().clone()
    }

    fn record(&self, event: MemEvent) {
        self.ledger.lock().push(event);
    }

    /// What one provider is allowed, when several are active.
    ///
    /// Proportional to declared priority, so equal priorities are equal shares.
    /// See open question 1 in `12-memory.md`.
    fn share_for(&self, provider: &dyn MemoryProvider) -> TokenBudget {
        let total: u64 = self
            .providers
            .iter()
            .map(|p| u64::from(p.priority().max(1)))
            .sum();
        if total == 0 {
            return TokenBudget { max: 0, reserve: 0 };
        }
        let available = self.allowance.available();
        let mine = u64::from(provider.priority().max(1));
        TokenBudget {
            max: available.saturating_mul(mine) / total,
            reserve: 0,
        }
    }

    /// `context.build`'s door. One [`Recall`] per provider that contributed.
    ///
    /// Never fails: a provider that errors is recorded and skipped, because a
    /// turn that dies because somebody's memory backend is down is a turn lost
    /// to something that was never load-bearing.
    pub async fn recall(&self, actor: &Actor, query: &str) -> Vec<Recall> {
        let readable = actor.readable_scopes(&self.lifetimes);
        let mut out = Vec::new();
        for provider in &self.providers {
            let kept: Vec<MemScope> = readable
                .iter()
                .filter(|s| provider.scopes().contains(&s.kind()))
                .filter(|s| {
                    self.permissions
                        .check(&actor.subject, &actor.agent, Aspect::MemRead, s)
                        .is_ok()
                })
                .cloned()
                .collect();
            if kept.is_empty() {
                continue;
            }
            let budget = self.share_for(provider.as_ref());
            let scopes = kept.len();
            let answered = provider
                .recall(RecallQuery {
                    scopes: kept,
                    query: query.to_owned(),
                    budget,
                })
                .await;
            let entries = match answered {
                Ok(entries) => entries,
                Err(e) => {
                    self.record(MemEvent::Failed {
                        provider: provider.id().to_owned(),
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            let clamped = clamp(entries, budget, self.counter.as_ref());
            if clamped.dropped > 0 {
                self.record(MemEvent::Clipped {
                    provider: provider.id().to_owned(),
                    dropped: clamped.dropped,
                    allowance: clamped.allowance,
                    used: clamped.used_tokens,
                });
            }
            if clamped.entries.is_empty() {
                continue;
            }
            self.record(MemEvent::Recalled {
                provider: provider.id().to_owned(),
                scopes,
                entries: clamped.entries.len(),
                tokens: clamped.used_tokens,
            });
            out.push(Recall {
                provider: provider.id().to_owned(),
                entries: clamped.entries.iter().map(MemEntry::to_recalled).collect(),
                dropped: clamped.dropped,
                used_tokens: clamped.used_tokens,
                allowance: clamped.allowance,
            });
        }
        out
    }

    /// Check the three things that are the kernel's, in the order that makes the
    /// refusal name the real reason.
    fn admit(&self, actor: &Actor, aspect: Aspect, scope: &MemScope) -> Result<(), MemError> {
        let request = request_text(aspect, scope);
        let refuse = |reason: String| {
            self.record(MemEvent::Refused {
                request: request.clone(),
                scope: scope.to_string(),
                reason: reason.clone(),
            });
            Err(MemError::denied(request.clone(), reason))
        };

        let allowed = match aspect {
            Aspect::MemWrite => actor.can_write(scope),
            _ => actor.can_read(scope),
        };
        if !allowed {
            return refuse(match aspect {
                Aspect::MemWrite => actor.refuse_write(scope),
                _ => format!("`{scope}` is not on `{}`'s own chain", actor.agent.agent),
            });
        }
        if !self.lifetimes.is_live(scope) {
            return refuse(format!("`{scope}` has ended"));
        }
        if let Err(reason) = self
            .permissions
            .check(&actor.subject, &actor.agent, aspect, scope)
        {
            return refuse(reason);
        }
        Ok(())
    }

    /// Write an entry. **Lifecycle handlers only**; the witness is the proof.
    ///
    /// # Errors
    ///
    /// [`MemError::Denied`] when the visibility rule, a dead scope or a
    /// permission rule refuses — in which case no provider is reached at all.
    /// [`MemError::UnsupportedScope`] when no active provider keeps this scope.
    pub async fn write(
        &self,
        witness: &LifecycleWitness,
        actor: &Actor,
        scope: MemScope,
        entry: MemEntry,
    ) -> Result<(), MemError> {
        self.admit(actor, Aspect::MemWrite, &scope)?;
        let mut written = 0usize;
        let mut last: Option<MemError> = None;
        for provider in &self.providers {
            if !provider.scopes().contains(&scope.kind()) {
                continue;
            }
            match provider.write(witness, scope.clone(), entry.clone()).await {
                Ok(()) => {
                    written += 1;
                    self.record(MemEvent::Wrote {
                        provider: provider.id().to_owned(),
                        scope: scope.to_string(),
                        key: entry.key.clone(),
                    });
                }
                Err(e) => {
                    self.record(MemEvent::Failed {
                        provider: provider.id().to_owned(),
                        message: e.to_string(),
                    });
                    last = Some(e);
                }
            }
        }
        if written > 0 {
            return Ok(());
        }
        Err(last.unwrap_or(MemError::UnsupportedScope {
            provider: "<no active provider>".to_owned(),
            scope: scope.kind().name().to_owned(),
        }))
    }

    /// Forget entries. **Lifecycle handlers only.** Returns how many went.
    ///
    /// # Errors
    ///
    /// As [`write`](Self::write).
    pub async fn forget(
        &self,
        witness: &LifecycleWitness,
        actor: &Actor,
        scope: MemScope,
        selector: MemSelector,
    ) -> Result<u64, MemError> {
        self.admit(actor, Aspect::MemWrite, &scope)?;
        let mut removed = 0u64;
        for provider in &self.providers {
            if !provider.scopes().contains(&scope.kind()) {
                continue;
            }
            match provider
                .forget(witness, scope.clone(), selector.clone())
                .await
            {
                Ok(n) => {
                    removed += n;
                    self.record(MemEvent::Forgot {
                        provider: provider.id().to_owned(),
                        scope: scope.to_string(),
                        removed: n,
                    });
                }
                Err(e) => self.record(MemEvent::Failed {
                    provider: provider.id().to_owned(),
                    message: e.to_string(),
                }),
            }
        }
        Ok(removed)
    }

    /// Discard the bytes of every scope that has already stopped resolving.
    ///
    /// Clearing is **lazy**: a retired scope is invisible from the instant its
    /// guard drops, and this is what eventually reclaims the space. It is a
    /// lifecycle handler's job — hence the witness — and it goes through the
    /// providers directly, because the actor it would be checked against is
    /// exactly the thing that has ended.
    pub async fn sweep(&self, witness: &LifecycleWitness) -> u64 {
        let mut removed = 0u64;
        for scope in self.lifetimes.pending_sweep() {
            let mut swept_all = true;
            for provider in &self.providers {
                if !provider.scopes().contains(&scope.kind()) {
                    continue;
                }
                match provider
                    .forget(witness, scope.clone(), MemSelector::All)
                    .await
                {
                    Ok(n) => {
                        removed += n;
                        self.record(MemEvent::Forgot {
                            provider: provider.id().to_owned(),
                            scope: scope.to_string(),
                            removed: n,
                        });
                    }
                    Err(e) => {
                        swept_all = false;
                        self.record(MemEvent::Failed {
                            provider: provider.id().to_owned(),
                            message: e.to_string(),
                        });
                    }
                }
            }
            if swept_all {
                self.lifetimes.swept(&scope);
            }
        }
        removed
    }
}
