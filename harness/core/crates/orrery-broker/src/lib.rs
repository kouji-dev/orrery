//! The only path to the outside world: read, write, spawn, net and creds behind a capability token.
//!
//! # Say plainly which layer enforces
//!
//! Claude Code's docs are candid that a `Bash(curl *)` deny stops
//! `curl https://x` but not `/usr/bin/curl https://x` or
//! `sh -c 'curl https://x'`. Patterns describe intent; **they do not enforce**.
//!
//! So: **rules decide whether to ask; the broker and the token decide what can
//! be touched.** A `spawn` grant is enforced where the process is created — in
//! [`proc`], inside a job object or a process group — and not by the string that
//! matched. A `read` grant is enforced against the token's resolved path, not
//! against the pattern that produced it. Anything that reads a rule in
//! `orrery-policy` as a guarantee has misread it; the guarantee is here.
//!
//! # Budgets are enforced where the resource is
//!
//! Output ceilings apply **while** the bytes are produced ([`limit`]), not to the
//! result. Wall clock is a watchdog on the process, not a timer somebody checks
//! afterwards. Memory is the platform's, with [`contain::MemoryEnforcement`]
//! saying honestly whether the platform really enforces it.
//!
//! # Credentials are used, never read
//!
//! [`Broker::creds`] resolves a **name** at the point of use and never returns
//! the value. Rotation rewrites what the name resolves to and nothing holding a
//! reference changes. There is therefore nothing for the audit to redact.
//!
//! Implementation plan: `harness/docs/plans/07-policy-broker-audit.md`

#![deny(missing_docs)]
// This crate genuinely needs `unsafe` (see harness/docs/plans/00b-scaffold-workspace.md,
// open question 1): every `unsafe` block carries a `// SAFETY:` comment.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod contain;
pub mod creds;
pub mod error;
pub mod fs;
pub mod gate;
pub mod limit;
pub mod net;
pub mod proc;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use orrery_audit::{Audit, AuditEvent, CallOutcome};
use orrery_policy::{CapabilityToken, TokenLedger};
use orrery_proto::Aspect;
use orrery_tools::ToolBudget;

pub use contain::MemoryEnforcement;
pub use creds::{CredStore, CredUse, FileCredStore, MemoryCredStore, RequestSlot};
pub use error::BrokerError;
pub use fs::WriteHandle;
pub use gate::{EngineGate, grant_for};
pub use limit::{LimitedReader, PullCounter};
pub use net::{NetRequest, NetResponse, NetTransport, NoTransport};
pub use proc::{Child, Output, SpawnSpec};

/// Everything that reaches outside the process.
///
/// Every method takes a [`CapabilityToken`] **by value**, so a token is spent by
/// the call itself and cannot be reused even before the ledger is consulted.
#[async_trait]
pub trait Broker: Send + Sync {
    /// Read a path, bounded while reading.
    ///
    /// # Errors
    ///
    /// When the token is not good, is for something else, or the read fails.
    async fn read(
        &self,
        t: CapabilityToken,
        path: &Path,
        budget: &ToolBudget,
    ) -> Result<LimitedReader<tokio::fs::File>, BrokerError>;

    /// Open a path for writing, optionally all-or-nothing.
    ///
    /// # Errors
    ///
    /// When the token is not good, or the file cannot be opened.
    async fn write(
        &self,
        t: CapabilityToken,
        path: &Path,
        atomic: bool,
    ) -> Result<WriteHandle, BrokerError>;

    /// Create a process, contained and watched.
    ///
    /// # Errors
    ///
    /// When the token is not good, or the process cannot be started or contained.
    async fn spawn(
        &self,
        t: CapabilityToken,
        cmd: SpawnSpec,
        budget: &ToolBudget,
    ) -> Result<Child, BrokerError>;

    /// Send a request, if a transport has been installed.
    ///
    /// # Errors
    ///
    /// [`BrokerError::NoTransport`] by default: the broker does not dial on its
    /// own.
    async fn net(
        &self,
        t: CapabilityToken,
        req: NetRequest,
        budget: &ToolBudget,
    ) -> Result<NetResponse, BrokerError>;

    /// Resolve a **name** at the point of use and never return the value.
    ///
    /// # Errors
    ///
    /// When the token is not good, or nothing is stored under that name.
    async fn creds(
        &self,
        t: CapabilityToken,
        name: &str,
        use_it: CredUse,
    ) -> Result<(), BrokerError>;
}

/// The broker this process actually runs.
#[derive(Debug)]
pub struct LocalBroker {
    ledger: Arc<TokenLedger>,
    creds: Arc<dyn CredStore>,
    transport: Arc<dyn NetTransport>,
    audit: Audit,
}

impl LocalBroker {
    /// A broker over the engine's ledger.
    ///
    /// It redeems against the same ledger the policy engine mints into, which is
    /// what makes a revoked call's in-flight token fail on its next use.
    #[must_use]
    pub fn new(ledger: Arc<TokenLedger>) -> Self {
        Self {
            ledger,
            creds: Arc::new(MemoryCredStore::new()),
            transport: Arc::new(NoTransport),
            audit: orrery_audit::null(),
        }
    }

    /// Use a particular credential store.
    #[must_use]
    pub fn with_creds(mut self, creds: Arc<dyn CredStore>) -> Self {
        self.creds = creds;
        self
    }

    /// Install a network transport. Without one, [`Broker::net`] refuses.
    #[must_use]
    pub fn with_transport(mut self, transport: Arc<dyn NetTransport>) -> Self {
        self.transport = transport;
        self
    }

    /// Record every brokered call.
    #[must_use]
    pub fn with_audit(mut self, audit: Audit) -> Self {
        self.audit = audit;
        self
    }

    /// The credential store, so a caller can rotate a value.
    #[must_use]
    pub fn creds_store(&self) -> &Arc<dyn CredStore> {
        &self.creds
    }

    /// Redeem a token and check it is for this call.
    ///
    /// This is the enforcement point. Not the pattern that matched: this.
    fn redeem(
        &self,
        token: &CapabilityToken,
        wanted: Aspect,
        target: &str,
    ) -> Result<(), BrokerError> {
        self.ledger.redeem(token.nonce())?;
        if token.aspect() != wanted {
            return Err(BrokerError::WrongAspect {
                held: token.aspect(),
                wanted,
            });
        }
        if !scope_covers(&token.scope().target, target) {
            return Err(BrokerError::OutsideScope {
                held: token.scope().target.clone(),
                wanted: target.to_owned(),
            });
        }
        Ok(())
    }

    fn record(&self, token: &CapabilityToken, what: &str, outcome: CallOutcome) {
        self.audit.append(AuditEvent::ToolCall {
            call: token.call(),
            tool: what.to_owned(),
            input: orrery_audit::Digest::of_bytes(token.scope().target.as_bytes()),
            outcome,
        });
    }
}

/// Whether a token minted for `held` covers `wanted`.
///
/// Paths are compared after the same normalisation the policy engine used to
/// resolve them, so `./src/main.rs` and an absolute path to the same file are
/// the same target. Everything else is an exact name.
fn scope_covers(held: &str, wanted: &str) -> bool {
    if held == wanted {
        return true;
    }
    let fold = |s: &str| {
        let s = s.replace('\\', "/");
        if cfg!(any(windows, target_os = "macos")) {
            s.to_lowercase()
        } else {
            s
        }
    };
    fold(held) == fold(wanted)
}

fn budget_duration(budget: &ToolBudget) -> Duration {
    Duration::from_millis(budget.wall_clock_ms.max(1))
}

#[async_trait]
impl Broker for LocalBroker {
    async fn read(
        &self,
        t: CapabilityToken,
        path: &Path,
        budget: &ToolBudget,
    ) -> Result<LimitedReader<tokio::fs::File>, BrokerError> {
        let normalised = orrery_policy::r#match::normalise_path(
            &path.display().to_string(),
            t.scope().root.as_deref().unwrap_or(Path::new("")),
        );
        self.redeem(&t, Aspect::Read, &normalised)?;
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|e| BrokerError::io(path, e))?;
        self.record(&t, "broker.read", CallOutcome::Ok);
        Ok(LimitedReader::new(file, budget.output_bytes))
    }

    async fn write(
        &self,
        t: CapabilityToken,
        path: &Path,
        atomic: bool,
    ) -> Result<WriteHandle, BrokerError> {
        let normalised = orrery_policy::r#match::normalise_path(
            &path.display().to_string(),
            t.scope().root.as_deref().unwrap_or(Path::new("")),
        );
        self.redeem(&t, Aspect::Write, &normalised)?;
        let handle = WriteHandle::open(path, atomic).await?;
        self.record(&t, "broker.write", CallOutcome::Ok);
        Ok(handle)
    }

    async fn spawn(
        &self,
        t: CapabilityToken,
        cmd: SpawnSpec,
        budget: &ToolBudget,
    ) -> Result<Child, BrokerError> {
        self.redeem(&t, Aspect::Spawn, &cmd.command_text())?;
        let child = Child::spawn(
            &cmd,
            budget_duration(budget),
            budget.output_bytes,
            budget.memory_bytes,
        )?;
        self.record(&t, "broker.spawn", CallOutcome::Ok);
        Ok(child)
    }

    async fn net(
        &self,
        t: CapabilityToken,
        req: NetRequest,
        budget: &ToolBudget,
    ) -> Result<NetResponse, BrokerError> {
        self.redeem(&t, Aspect::Net, &req.host())?;
        let (method, url, headers, body) = req.parts();
        let out = self
            .transport
            .send(&method, &url, &headers, &body, budget.output_bytes)
            .await;
        self.record(
            &t,
            "broker.net",
            if out.is_ok() {
                CallOutcome::Ok
            } else {
                CallOutcome::Failed
            },
        );
        out
    }

    async fn creds(
        &self,
        t: CapabilityToken,
        name: &str,
        use_it: CredUse,
    ) -> Result<(), BrokerError> {
        self.redeem(&t, Aspect::Creds, name)?;
        let out = self.creds.apply(name, &use_it);
        // The NAME is recorded. There is no value to record: `apply` did not
        // return one, and nothing in this function has ever seen it.
        self.record(
            &t,
            "broker.creds",
            if out.is_ok() {
                CallOutcome::Ok
            } else {
                CallOutcome::Failed
            },
        );
        out
    }
}
