//! Where a provider's secret lives: behind the `creds` grant.
//!
//! One definition, in the published crate, because every provider extension
//! needs the same three operations and two of them had already been written
//! twice. A community provider gets it by depending on this crate, and its
//! tests get [`MockBroker`](crate::testing::MockBroker) — the same ledger a
//! real session shows.
//!
//! # The grant is the point
//!
//! [`BrokerCredStore`] does not read a file, an environment variable or a
//! config key. It asks the broker for a **name**, which means the read is
//! policy-checked against `creds`, lands in the ledger, and is refused with a
//! rule id when no grant covers it. An extension that was not granted
//! `creds = ["anthropic"]` cannot reach the Anthropic key even though it is
//! running in the same process.
//!
//! # The one place a value materialises
//!
//! [`CredStore::get`] returns a `String`, and that is the only method in this
//! module that does. It exists because a provider has to put the secret in a
//! header immediately afterwards, and nothing in a published extension can
//! hold the socket. Everything else is arranged so that the value has exactly
//! one call site: no type here derives `Debug` over it, `MemoryCredStore` and
//! `BrokerCredStore` both print the *names* they hold and never the values, and
//! nothing stores the result.
//!
//! Implementation plan: `harness/docs/plans/07-policy-broker-audit.md`

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::broker::{BrokerError, BrokerFacade, BrokerResult};

/// Somewhere one named credential lives.
///
/// # Errors, and what is not one
///
/// A grant that is simply **not set** is `Ok(None)`: a missing credential is a
/// login prompt, not a failure. A grant the policy **refuses** is an `Err`, and
/// the two are deliberately different answers — collapsing them would send a
/// person to a login flow that cannot help them.
#[async_trait]
pub trait CredStore: Send + Sync {
    /// Read a grant.
    ///
    /// # Errors
    ///
    /// [`BrokerError::Denied`] when no `creds` grant covers the name, and
    /// [`BrokerError::Io`] when the store itself is unreachable.
    async fn get(&self, grant: &str) -> BrokerResult<Option<String>>;

    /// Write a grant.
    ///
    /// # Errors
    ///
    /// [`BrokerError::Unsupported`] when the store cannot hold secrets — the
    /// environment-variable fallback cannot, and says so instead of
    /// pretending.
    async fn put(&self, grant: &str, secret: &str) -> BrokerResult<()>;

    /// Forget a grant.
    ///
    /// # Errors
    ///
    /// As [`CredStore::put`].
    async fn clear(&self, grant: &str) -> BrokerResult<()>;

    /// Whether a grant is set, without reading it.
    ///
    /// This is what `state()` asks on every turn. A store that can hold a
    /// secret but not hand it back — an OS keychain, the broker — still
    /// answers this, which is why it is a method of its own rather than
    /// `get(..).is_some()`.
    ///
    /// # Errors
    ///
    /// As [`CredStore::get`].
    async fn has(&self, grant: &str) -> BrokerResult<bool> {
        Ok(self.get(grant).await?.is_some())
    }
}

/// The `creds` grant, as a store.
///
/// This is what a first-party provider is wired with. The broker behind it is
/// the call's own facade, so the check, the rule id and the ledger entry are
/// the session's rather than this type's.
pub struct BrokerCredStore {
    who: String,
    broker: Arc<dyn BrokerFacade>,
}

impl std::fmt::Debug for BrokerCredStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrokerCredStore")
            .field("who", &self.who)
            .finish_non_exhaustive()
    }
}

impl BrokerCredStore {
    /// A store over one call's broker. `who` names the holder in error
    /// messages — an extension id, usually.
    #[must_use]
    pub fn new(who: impl Into<String>, broker: Arc<dyn BrokerFacade>) -> Self {
        Self {
            who: who.into(),
            broker,
        }
    }
}

#[async_trait]
impl CredStore for BrokerCredStore {
    async fn get(&self, grant: &str) -> BrokerResult<Option<String>> {
        match self.broker.credential(grant).await {
            Ok(secret) => Ok(Some(secret)),
            // The broker distinguishes "you may not" from "there is none" by
            // the error it returns; only the second is a `None`.
            Err(BrokerError::Io { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn put(&self, grant: &str, secret: &str) -> BrokerResult<()> {
        self.broker.store_credential(grant, secret).await
    }

    async fn clear(&self, grant: &str) -> BrokerResult<()> {
        self.broker.forget_credential(grant).await
    }

    async fn has(&self, grant: &str) -> BrokerResult<bool> {
        self.broker.has_credential(grant).await
    }
}

/// A store backed by `ANTHROPIC_API_KEY` and friends.
///
/// **A development fallback, and deliberately kept.** It is not a TODO waiting
/// on the broker: the broker landed, [`BrokerCredStore`] is what a session
/// wires, and this exists for the two cases the grant cannot serve —
///
/// - a contributor running one provider's tests with a key they exported in
///   their own shell, before any session or policy exists, and
/// - CI for a downstream embedder, where the secret arrives as an environment
///   variable because that is what every CI system hands you.
///
/// It is **read-only**, on purpose: a login that wrote to the process
/// environment would be lost at exit and invisible to every other process,
/// which is worse than refusing. Anything that has to persist a token — the
/// OAuth flow, most of all — must be given a [`BrokerCredStore`], and will fail
/// loudly here rather than appear to work.
#[derive(Debug, Default, Clone, Copy)]
pub struct EnvCredStore;

impl EnvCredStore {
    /// The variable a grant is read from: `anthropic` → `ANTHROPIC_API_KEY`.
    #[must_use]
    pub fn var(grant: &str) -> String {
        format!("{}_API_KEY", grant.to_uppercase().replace('-', "_"))
    }
}

#[async_trait]
impl CredStore for EnvCredStore {
    async fn get(&self, grant: &str) -> BrokerResult<Option<String>> {
        Ok(std::env::var(Self::var(grant))
            .ok()
            .filter(|v| !v.trim().is_empty()))
    }

    async fn put(&self, grant: &str, _secret: &str) -> BrokerResult<()> {
        Err(BrokerError::Io {
            message: format!(
                "cannot store a credential in the environment: set {} instead, \
                 or wire a `BrokerCredStore` so the `creds` grant holds it",
                Self::var(grant)
            ),
        })
    }

    async fn clear(&self, grant: &str) -> BrokerResult<()> {
        Err(BrokerError::Io {
            message: format!(
                "cannot clear a credential in the environment: unset {}",
                Self::var(grant)
            ),
        })
    }
}

/// Two stores, tried in order: the grant first, the fallback second.
///
/// # Why a provider needs this today
///
/// The broker's credential store **stores** and **applies** a secret and never
/// returns one — `orrery_broker::CredStore` has `apply`, `store` and `has`, and
/// deliberately no `get`, which `orrery-broker`'s own `value_never_returned`
/// test pins. Applying it means the broker owns the outgoing request, and the
/// broker installs no HTTP transport yet (`fetch` answers "no transport is
/// installed"; plan 14 owns that). Until it does, a provider has to put the key
/// in a header it owns, so it has to *have* the key.
///
/// So: `login` and `logout` write through the grant, where the value belongs,
/// and reading falls through to [`EnvCredStore`]. When plan 14's transport
/// lands, the fallback is what goes, and nothing else in a provider changes —
/// which is why the seam is this type rather than an `if` in each of them.
pub struct LayeredCredStore {
    primary: Arc<dyn CredStore>,
    fallback: Arc<dyn CredStore>,
}

impl std::fmt::Debug for LayeredCredStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LayeredCredStore")
    }
}

impl LayeredCredStore {
    /// Read from `primary`, then `fallback`. Writes go to `primary` only: a
    /// write that silently landed in the fallback would be a token nobody can
    /// revoke.
    #[must_use]
    pub fn new(primary: Arc<dyn CredStore>, fallback: Arc<dyn CredStore>) -> Self {
        Self { primary, fallback }
    }

    /// The grant, with the environment behind it. What a session wires.
    #[must_use]
    pub fn over_broker(who: impl Into<String>, broker: Arc<dyn BrokerFacade>) -> Self {
        Self::new(
            Arc::new(BrokerCredStore::new(who, broker)),
            Arc::new(EnvCredStore),
        )
    }
}

#[async_trait]
impl CredStore for LayeredCredStore {
    async fn get(&self, grant: &str) -> BrokerResult<Option<String>> {
        // A *denial* from the primary is not a reason to try the fallback:
        // "you may not have this" has been answered, and reaching around it
        // through an environment variable is exactly the hole the grant exists
        // to close. Only "this store cannot do that" falls through.
        match self.primary.get(grant).await {
            Ok(Some(v)) => Ok(Some(v)),
            Ok(None) | Err(BrokerError::Unsupported { .. }) => self.fallback.get(grant).await,
            Err(e) => Err(e),
        }
    }

    async fn put(&self, grant: &str, secret: &str) -> BrokerResult<()> {
        self.primary.put(grant, secret).await
    }

    async fn clear(&self, grant: &str) -> BrokerResult<()> {
        self.primary.clear(grant).await
    }

    async fn has(&self, grant: &str) -> BrokerResult<bool> {
        match self.primary.has(grant).await {
            Ok(true) => Ok(true),
            Ok(false) | Err(BrokerError::Unsupported { .. }) => self.fallback.has(grant).await,
            Err(e) => Err(e),
        }
    }
}

/// An in-process store, for tests and for a session that was handed its secrets
/// rather than told where to find them.
#[derive(Default)]
pub struct MemoryCredStore {
    grants: RwLock<BTreeMap<String, String>>,
}

impl std::fmt::Debug for MemoryCredStore {
    /// Names, never values. A derived `Debug` would put every secret it holds
    /// into whichever log line formatted the store.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryCredStore")
            .field("grants", &self.grants.read().keys().collect::<Vec<_>>())
            .finish()
    }
}

#[async_trait]
impl CredStore for MemoryCredStore {
    async fn get(&self, grant: &str) -> BrokerResult<Option<String>> {
        Ok(self.grants.read().get(grant).cloned())
    }

    async fn put(&self, grant: &str, secret: &str) -> BrokerResult<()> {
        self.grants
            .write()
            .insert(grant.to_owned(), secret.to_owned());
        Ok(())
    }

    async fn clear(&self, grant: &str) -> BrokerResult<()> {
        self.grants.write().remove(grant);
        Ok(())
    }
}
