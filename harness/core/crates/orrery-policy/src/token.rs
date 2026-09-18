//! The capability token: unforgeable, single-use, revocable, and **not
//! `Serialize`**.
//!
//! Three properties, each a test:
//!
//! 1. **Unforgeable.** The one field is private and so is the tuple-struct
//!    constructor, so no crate outside this one can make a token. The
//!    `compile_fail` doctests on [`CapabilityToken`] prove it from the outside,
//!    which is where it matters.
//! 2. **Single-use.** The broker takes a token **by value**, so the move alone
//!    consumes it; the nonce redemption in [`TokenLedger`] catches any second
//!    attempt.
//! 3. **Revocable.** Cancelling a call drops its nonces, so an in-flight tool's
//!    next broker call fails [`TokenError::Revoked`] rather than succeeding on a
//!    grant nobody wants any more.
//!
//! And the load-bearing one: it is not `Serialize`, so section 4.8's "no
//! extension ever holds a handle" is a **type error** rather than a code review.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::{DashMap, DashSet};
use orrery_proto::{Aspect, CallId, RuleId};

use crate::error::TokenError;

/// How long a freshly minted token is good for, unless something says otherwise.
pub const DEFAULT_TTL: Duration = Duration::from_secs(300);

/// What a token actually permits, resolved: no patterns left to interpret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedScope {
    /// The exact target — a normalised path, a host, a command, a tool id.
    pub target: String,
    /// The workspace root the target was resolved against, for path aspects.
    pub root: Option<std::path::PathBuf>,
}

impl ResolvedScope {
    /// A scope over one resolved target.
    #[must_use]
    pub fn new(target: impl Into<String>) -> Self {
        Self {
            target: target.into(),
            root: None,
        }
    }

    /// Note the root the target was resolved against.
    #[must_use]
    pub fn under(mut self, root: impl Into<std::path::PathBuf>) -> Self {
        self.root = Some(root.into());
        self
    }
}

#[derive(Debug)]
struct Inner {
    call: CallId,
    aspect: Aspect,
    scope: ResolvedScope,
    deadline: Instant,
    nonce: u64,
    rule: RuleId,
}

/// Permission to do one thing, once.
///
/// # It cannot be constructed from outside this crate
///
/// ```compile_fail
/// // The type is public; its constructor is not.
/// let _make_one = orrery_policy::CapabilityToken;
/// ```
///
/// # It cannot be serialised
///
/// ```compile_fail
/// fn needs_serde<T: serde::Serialize>() {}
/// needs_serde::<orrery_policy::CapabilityToken>();
/// ```
///
/// Naming the type, by contrast, compiles fine — so neither failure above is a
/// typo passing for a proof:
///
/// ```
/// fn takes_one(_t: orrery_policy::CapabilityToken) {}
/// ```
#[derive(Debug)]
pub struct CapabilityToken(Inner);

impl CapabilityToken {
    /// The call this was minted for.
    #[must_use]
    pub fn call(&self) -> CallId {
        self.0.call
    }

    /// What it permits.
    #[must_use]
    pub fn aspect(&self) -> Aspect {
        self.0.aspect
    }

    /// The resolved target.
    #[must_use]
    pub fn scope(&self) -> &ResolvedScope {
        &self.0.scope
    }

    /// The rule that produced it, so the audit can name it.
    #[must_use]
    pub fn rule(&self) -> RuleId {
        self.0.rule
    }

    /// Its one-time nonce.
    #[must_use]
    pub fn nonce(&self) -> u64 {
        self.0.nonce
    }

    /// When it stops being good.
    #[must_use]
    pub fn deadline(&self) -> Instant {
        self.0.deadline
    }
}

/// The one place tokens come from.
///
/// Constructible only at boot, from a [`TokenLedger`]; [`TokenMinter::mint`] is
/// crate-private, so the engine mints and nobody else does.
#[derive(Debug, Clone)]
pub struct TokenMinter {
    ledger: Arc<TokenLedger>,
    ttl: Duration,
}

impl TokenMinter {
    /// A minter over a ledger. Called once, at boot.
    #[must_use]
    pub fn new(ledger: Arc<TokenLedger>) -> Self {
        Self {
            ledger,
            ttl: DEFAULT_TTL,
        }
    }

    /// Mint with a different lifetime.
    #[must_use]
    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// The ledger this mints into.
    #[must_use]
    pub fn ledger(&self) -> &Arc<TokenLedger> {
        &self.ledger
    }

    pub(crate) fn mint(
        &self,
        call: CallId,
        aspect: Aspect,
        scope: ResolvedScope,
        rule: RuleId,
    ) -> CapabilityToken {
        let deadline = Instant::now() + self.ttl;
        let nonce = self.ledger.issue(call, deadline);
        CapabilityToken(Inner {
            call,
            aspect,
            scope,
            deadline,
            nonce,
            rule,
        })
    }
}

#[derive(Debug, Copy, Clone)]
struct Live {
    call: CallId,
    deadline: Instant,
}

/// Nonces are drawn from one process-wide counter, so two ledgers never issue
/// the same number and a token from one can never be mistaken for a token from
/// another.
static NEXT_NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Which nonces are still good, and which calls have been cancelled.
#[derive(Debug, Default)]
pub struct TokenLedger {
    live: DashMap<u64, Live>,
    revoked: DashSet<u64>,
    /// The first and last nonce this ledger issued, so a nonce it has already
    /// taken back can be told from one it never had, without keeping every
    /// nonce it ever issued.
    first: std::sync::atomic::AtomicU64,
    last: std::sync::atomic::AtomicU64,
}

impl TokenLedger {
    /// An empty ledger.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn issue(&self, call: CallId, deadline: Instant) -> u64 {
        use std::sync::atomic::Ordering::SeqCst;
        // Monotonic rather than random: a nonce never leaves the process, and a
        // counter cannot collide.
        let nonce = NEXT_NONCE.fetch_add(1, SeqCst);
        let _ = self.first.compare_exchange(0, nonce, SeqCst, SeqCst);
        self.last.store(nonce, SeqCst);
        self.live.insert(nonce, Live { call, deadline });
        nonce
    }

    /// Check and remove, in one step.
    ///
    /// # Errors
    ///
    /// [`TokenError::Spent`] on a second redemption, [`TokenError::Revoked`]
    /// when the call was cancelled, [`TokenError::Expired`] past the deadline,
    /// [`TokenError::Unknown`] for a nonce this ledger never issued.
    pub fn redeem(&self, nonce: u64) -> Result<(), TokenError> {
        if self.revoked.remove(&nonce).is_some() {
            return Err(TokenError::Revoked);
        }
        let Some((_, live)) = self.live.remove(&nonce) else {
            // A nonce this ledger did issue and has already taken back is
            // spent; one it never issued at all is unknown. The counter tells
            // them apart without keeping every nonce forever.
            use std::sync::atomic::Ordering::SeqCst;
            let (first, last) = (self.first.load(SeqCst), self.last.load(SeqCst));
            return Err(if first != 0 && nonce >= first && nonce <= last {
                TokenError::Spent
            } else {
                TokenError::Unknown
            });
        };
        if Instant::now() > live.deadline {
            return Err(TokenError::Expired);
        }
        Ok(())
    }

    /// Cancelling a call drops its nonces, so anything still in flight fails
    /// [`TokenError::Revoked`] on its next broker call.
    pub fn revoke_call(&self, call: CallId) {
        let doomed: Vec<u64> = self
            .live
            .iter()
            .filter(|e| e.value().call == call)
            .map(|e| *e.key())
            .collect();
        for nonce in doomed {
            self.live.remove(&nonce);
            self.revoked.insert(nonce);
        }
    }

    /// How many tokens are still redeemable.
    #[must_use]
    pub fn live(&self) -> usize {
        self.live.len()
    }
}
