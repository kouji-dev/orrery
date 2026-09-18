//! What the guest may ask for, and what the host answers.
//!
//! # Why this is a trait and not `orrery_broker::Broker`
//!
//! [`orrery_broker::Broker`] takes a `CapabilityToken` **by value** on every
//! method — that is plan 07's whole design, and it is right. But a token cannot
//! cross into guest memory, so the wasm host needs a face that has no token in
//! it at all. [`HostBroker`] is that face:
//!
//! > The guest asks. The host decides. The token is looked up on the host side,
//! > by the embedder that implements this trait, from the current call.
//!
//! That is objective 2, enforced by the ABI rather than by discipline: there is
//! no signature here a token could be smuggled through, so
//! `imports::guest_never_sees_a_token` is a type-level fact and not a review
//! note. The real implementation — the one that mints against the policy
//! engine's ledger and calls `LocalBroker` — is the embedder's, because minting
//! is the policy engine's privilege and this crate does not have it.

use async_trait::async_trait;

pub use crate::bindings::orrery::extension::broker::{
    CredUsage, FetchOpts, FetchOut, FileOut, ProcOut, RunOpts,
};

/// Why an operation did not happen.
///
/// The same four cases as the `.wit` `variant error`, and the same rule: these
/// are **values**, handed to the guest to branch on. Nothing here traps.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Failure {
    /// A policy rule said no. Terminal — retrying will not help.
    #[error("denied: {0}")]
    Denied(String),
    /// A budget ceiling was reached.
    #[error("over budget: {0}")]
    Budget(String),
    /// It was attempted and the platform refused.
    #[error("{0}")]
    Io(String),
    /// The turn was cancelled. Work stopped; it was not undone.
    #[error("cancelled")]
    Cancelled,
}

impl From<&orrery_broker::BrokerError> for Failure {
    /// Map the real broker's refusals onto the four the guest can see.
    ///
    /// A bad token is a **denial** from the guest's point of view, not an I/O
    /// error: it means the call was not permitted, whatever the mechanism.
    fn from(e: &orrery_broker::BrokerError) -> Self {
        use orrery_broker::BrokerError as B;
        match e {
            B::Token(_) | B::WrongAspect { .. } | B::OutsideScope { .. } => {
                Failure::Denied(e.to_string())
            }
            B::LimitExceeded { .. } | B::TimedOut { .. } => Failure::Budget(e.to_string()),
            B::Cancelled => Failure::Cancelled,
            _ => Failure::Io(e.to_string()),
        }
    }
}

impl From<Failure> for crate::bindings::orrery::extension::broker::Error {
    fn from(f: Failure) -> Self {
        use crate::bindings::orrery::extension::broker::Error as W;
        match f {
            Failure::Denied(why) => W::Denied(why),
            Failure::Budget(why) => W::Budget(why),
            Failure::Io(why) => W::Io(why),
            Failure::Cancelled => W::Cancelled,
        }
    }
}

/// Everything an extension can reach outside itself.
///
/// One method per import in the world. **No method takes a token**, and that is
/// the point: see the module docs.
#[async_trait]
pub trait HostBroker: Send + Sync {
    /// Spawn a process under the call's containment and budget.
    async fn run_proc(
        &self,
        cmd: &str,
        args: &[String],
        opts: RunOpts,
    ) -> Result<ProcOut, Failure>;

    /// Read a file, truncated at `max_bytes`. Never reads to end.
    async fn read_file(&self, path: &str, max_bytes: u64) -> Result<FileOut, Failure>;

    /// Write a file, optionally all-or-nothing.
    async fn write_file(&self, path: &str, bytes: &[u8], atomic: bool) -> Result<(), Failure>;

    /// Make one outbound request.
    async fn fetch(&self, url: &str, opts: FetchOpts) -> Result<FetchOut, Failure>;

    /// Attach a credential to an upcoming call. The secret never enters guest
    /// memory — only the answer to "may I" does.
    async fn use_credential(&self, name: &str, usage: CredUsage) -> Result<(), Failure>;
}

/// A broker that refuses everything, with a reason.
///
/// The default a host should fall back to, and what most tests start from: an
/// extension granted nothing must still *run*, and must still get values back
/// rather than traps.
#[derive(Debug, Clone)]
pub struct DenyAll {
    reason: String,
}

impl DenyAll {
    /// Refuse with this reason.
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl Default for DenyAll {
    fn default() -> Self {
        Self::new("this extension holds no grant for that")
    }
}

#[async_trait]
impl HostBroker for DenyAll {
    async fn run_proc(&self, _: &str, _: &[String], _: RunOpts) -> Result<ProcOut, Failure> {
        Err(Failure::Denied(self.reason.clone()))
    }
    async fn read_file(&self, _: &str, _: u64) -> Result<FileOut, Failure> {
        Err(Failure::Denied(self.reason.clone()))
    }
    async fn write_file(&self, _: &str, _: &[u8], _: bool) -> Result<(), Failure> {
        Err(Failure::Denied(self.reason.clone()))
    }
    async fn fetch(&self, _: &str, _: FetchOpts) -> Result<FetchOut, Failure> {
        Err(Failure::Denied(self.reason.clone()))
    }
    async fn use_credential(&self, _: &str, _: CredUsage) -> Result<(), Failure> {
        Err(Failure::Denied(self.reason.clone()))
    }
}
