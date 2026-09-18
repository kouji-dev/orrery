//! The join between the extension-facing facade and the real broker.
//!
//! An extension holds an [`Arc<dyn BrokerFacade>`](orrery_ext_api::BrokerFacade)
//! and nothing else. Behind it, every call goes:
//!
//! ```text
//! builtin.read ─> PolicyBroker ─> PolicyEngine::check ─> CapabilityToken
//!                              ─> LocalBroker::read ─> redeem ─> the file
//! ```
//!
//! The token is the enforcement point. Not the rule that matched, not the
//! pattern in the manifest: the broker redeems a token minted for one aspect
//! over one resolved target, and a mismatch is a refusal.
//!
//! # Two things it does, that it once could not
//!
//! - **It knows which call it is serving.** `ExtensionTable` asks this type,
//!   as a [`BrokerSource`], for a facade per dispatch, so
//!   [`PolicyBroker::for_call`] ties every token it mints to that call's id and
//!   to the token that stops it. `TokenLedger::revoke_call` therefore reaches a
//!   tool call that is still running, which is the whole point of revocation.
//! - **It lists directories.** [`BrokerFacade::list`] is how a tool discovers
//!   names, so a walk no longer has to reach around the broker with
//!   `std::fs::read_dir` — and a path this call may not read is not named in
//!   the answer.
//!
//! A listing mints no token. It hands back no bytes and no handle: what it
//! answers is which names this call would be allowed to read, and the read
//! itself is checked and redeemed when it happens.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use orrery_broker::{Broker, LocalBroker, SpawnSpec};
use orrery_ext_api::{
    BrokerError, BrokerFacade, BrokerResult, BrokerSource, ListEntry, ListRequest, Listing,
    NetRequest, NetResponse, ReadChunk, ReadRequest, SpawnOutput, SpawnRequest, WriteRequest,
};
use orrery_kernel::CallRevoker;
use orrery_policy::{
    CapabilityToken, Decision, PendingCall, PolicyEngine, TokenLedger, Verdict,
};
use orrery_proto::{AgentScope, CallId, CancelReason, Subject};
use orrery_tools::ToolBudget;
use tokio_util::sync::CancellationToken;

/// How many bytes go out at a time, and therefore how often a cancelled write
/// notices.
const WRITE_CHUNK: usize = 64 * 1024;

/// A [`BrokerFacade`] that asks the policy engine first.
pub struct PolicyBroker {
    engine: Arc<PolicyEngine>,
    broker: Arc<LocalBroker>,
    workspace: PathBuf,
    subject: Subject,
    scope: AgentScope,
    budget: ToolBudget,
    call: Option<CallId>,
    cancel: Option<CancellationToken>,
    /// The most bytes any one read has pulled from a file.
    ///
    /// Shared across [`PolicyBroker::for_call`] copies, because it is a property
    /// of the session's reads and not of one call's. It exists because "the
    /// ceiling bounds **peak** memory" is the claim worth checking, and a
    /// result-sized assertion would pass for an implementation that buffered
    /// the whole file and then truncated it.
    peak_pull: Arc<AtomicU64>,
}

impl std::fmt::Debug for PolicyBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PolicyBroker")
            .field("workspace", &self.workspace)
            .field("subject", &self.subject)
            .field("call", &self.call)
            .finish_non_exhaustive()
    }
}

impl PolicyBroker {
    /// A broker for one session.
    #[must_use]
    pub fn new(
        engine: Arc<PolicyEngine>,
        broker: Arc<LocalBroker>,
        workspace: impl Into<PathBuf>,
        subject: Subject,
        scope: AgentScope,
        budget: ToolBudget,
    ) -> Arc<Self> {
        Arc::new(Self {
            engine,
            broker,
            workspace: workspace.into(),
            subject,
            scope,
            budget,
            call: None,
            cancel: None,
            peak_pull: Arc::new(AtomicU64::new(0)),
        })
    }

    /// The most bytes any one read pulled from its source.
    #[must_use]
    pub fn peak_pulled(&self) -> u64 {
        self.peak_pull.load(Ordering::SeqCst)
    }

    /// The same broker, tied to one call.
    ///
    /// Tokens it mints are revoked when that call is, and a cancelled call's
    /// next write is refused rather than committed.
    #[must_use]
    pub fn for_call(&self, call: CallId, cancel: CancellationToken) -> Arc<Self> {
        Arc::new(Self {
            engine: self.engine.clone(),
            broker: self.broker.clone(),
            workspace: self.workspace.clone(),
            subject: self.subject.clone(),
            scope: self.scope.clone(),
            budget: self.budget,
            call: Some(call),
            cancel: Some(cancel),
            peak_pull: Arc::clone(&self.peak_pull),
        })
    }

    /// Resolve a path the way the policy engine will.
    ///
    /// A tool says `Cargo.toml` and means "in the workspace". Opening it
    /// relative to the process's current directory would check one path and open
    /// another, which is the shape of every sandbox escape ever written.
    fn resolve(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.workspace.join(path)
        }
    }

    /// Ask the engine, and keep the token it mints.
    fn allow(&self, call: PendingCall) -> BrokerResult<CapabilityToken> {
        let call = match self.call {
            Some(id) => call.in_call(id),
            None => call,
        };
        match self.engine.check(&call, &self.subject, &self.scope) {
            Decision::Allow { token, .. } => Ok(token),
            Decision::Deny { rule, reason } => Err(BrokerError::Denied { rule, reason }),
            // An `ask` that reaches here has nobody to answer it: the kernel
            // turns a `Decision::Ask` into a consent frame long before a tool
            // runs, so by this point the question has been settled.
            other => Err(BrokerError::Denied {
                rule: other.rule(),
                reason: "this call needs consent, and none was given".to_owned(),
            }),
        }
    }

    /// Whether this call would be allowed to read this path.
    ///
    /// [`PolicyEngine::explain`] rather than [`PolicyEngine::check`] on
    /// purpose: a listing is a question, not an action, and `check` would mint
    /// a capability token per directory entry that nothing would ever redeem.
    fn may_read(&self, path: &Path) -> bool {
        let call = PendingCall::read(path.display().to_string());
        self.engine.explain(&call, &self.subject).verdict == Verdict::Allow
    }

    fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
    }
}

fn io(e: impl std::fmt::Display) -> BrokerError {
    BrokerError::Io {
        message: e.to_string(),
    }
}

#[async_trait]
impl BrokerFacade for PolicyBroker {
    async fn read(&self, req: ReadRequest) -> BrokerResult<ReadChunk> {
        let path = self.resolve(&req.path);
        let token = self.allow(PendingCall::read(path.display().to_string()))?;
        // The ceiling is applied **while** reading: `LimitedReader` never pulls
        // more than it plus one buffer, so peak memory is bounded and not just
        // the result.
        let ceiling = req.limit.min(self.budget.output_bytes).max(1);
        let budget = ToolBudget::new(self.budget.wall_clock_ms, ceiling);
        let mut reader = self.broker.read(token, &path, &budget).await.map_err(io)?;
        let counter = reader.counter();
        let (bytes, truncated) = reader.take_bytes().await.map_err(io)?;
        self.peak_pull.fetch_max(counter.pulled(), Ordering::SeqCst);
        let total = tokio::fs::metadata(&path).await.ok().map(|m| m.len());
        Ok(ReadChunk {
            bytes,
            eof: !truncated,
            total,
        })
    }

    async fn list(&self, req: ListRequest) -> BrokerResult<Listing> {
        let root = self.resolve(&req.path);
        let limit = usize::try_from(req.limit).unwrap_or(usize::MAX).max(1);
        let mut entries: Vec<ListEntry> = Vec::new();
        let mut truncated = false;
        let mut queue = vec![root];

        'walk: while let Some(dir) = queue.pop() {
            // A directory that cannot be listed is one this call cannot see,
            // which is the same answer a denied read gives.
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in read.flatten() {
                let path = entry.path();
                if !self.may_read(&path) {
                    // Omitted, not refused: a name is information too.
                    continue;
                }
                let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
                if is_dir && req.recursive {
                    queue.push(path.clone());
                }
                let size = if is_dir {
                    None
                } else {
                    entry.metadata().ok().map(|m| m.len())
                };
                entries.push(ListEntry { path, is_dir, size });
                if entries.len() >= limit {
                    truncated = true;
                    break 'walk;
                }
            }
        }

        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Listing { entries, truncated })
    }

    async fn write(&self, req: WriteRequest) -> BrokerResult<()> {
        let path = self.resolve(&req.path);
        let token = self.allow(PendingCall::write(path.display().to_string()))?;
        let mut handle = self
            .broker
            .write(token, &path, req.atomic)
            .await
            .map_err(io)?;
        // Chunked, with the cancel checked between chunks. A single `write_all`
        // of a large file would be uninterruptible, and "atomic on cancel" would
        // then mean "atomic unless the file is big", which is the wrong way
        // round.
        for chunk in req.contents.chunks(WRITE_CHUNK) {
            if self.cancelled() {
                // Dropping would do this anyway; saying it out loud is what
                // makes an atomic write revert rather than half-land.
                handle.cancel().await;
                return Err(BrokerError::Cancelled {
                    reason: CancelReason::User,
                });
            }
            handle.write_all(chunk).await.map_err(io)?;
            tokio::task::yield_now().await;
        }
        if self.cancelled() {
            handle.cancel().await;
            return Err(BrokerError::Cancelled {
                reason: CancelReason::User,
            });
        }
        handle.commit().await.map_err(io)
    }

    async fn spawn(&self, req: SpawnRequest) -> BrokerResult<SpawnOutput> {
        let mut spec = SpawnSpec::new(&req.program).args(req.args.clone());
        spec = spec.cwd(
            req.cwd
                .as_ref()
                .map_or_else(|| self.workspace.clone(), |c| self.resolve(c)),
        );
        let token = self.allow(PendingCall::spawn(spec.command_text()))?;
        let budget = ToolBudget {
            wall_clock_ms: req.timeout_ms.map_or(self.budget.wall_clock_ms, |ms| {
                ms.min(self.budget.wall_clock_ms)
            }),
            output_bytes: req.output_bytes.map_or(self.budget.output_bytes, |b| {
                b.min(self.budget.output_bytes)
            }),
            memory_bytes: self.budget.memory_bytes,
        };
        let child = self.broker.spawn(token, spec, &budget).await.map_err(io)?;
        let out = child.wait().await.map_err(io)?;
        Ok(SpawnOutput {
            status: out.code,
            stdout: out.stdout,
            stderr: out.stderr,
            truncated: out.truncated,
        })
    }

    async fn fetch(&self, req: NetRequest) -> BrokerResult<NetResponse> {
        // The broker does not dial by default and this facade installs no
        // transport, so this is a refusal with a reason rather than a silent
        // failure. Phase 1 ships no first-party tool that needs the network.
        let host = req
            .url
            .split("://")
            .nth(1)
            .and_then(|rest| rest.split('/').next())
            .unwrap_or(&req.url)
            .to_owned();
        let _token = self.allow(PendingCall::net(host))?;
        Err(BrokerError::Unsupported {
            what: "the network: no transport is installed",
        })
    }

    async fn credential(&self, name: &str) -> BrokerResult<String> {
        // `Broker::creds` resolves a name at the point of use and never returns
        // the value, which is the whole design: there is nothing here to hand
        // back, and a facade that returned a string would undo it.
        let _token = self.allow(PendingCall::creds(name))?;
        Err(BrokerError::Unsupported {
            what: "reading a credential value: the broker uses one, it never returns one",
        })
    }
}

impl BrokerSource for PolicyBroker {
    fn for_call(&self, call: CallId, cancel: CancellationToken) -> Arc<dyn BrokerFacade> {
        PolicyBroker::for_call(self, call, cancel)
    }
}

/// Takes back a cancelled call's capabilities.
///
/// The kernel cancels a call; this is what makes that mean something to a tool
/// already holding a token — its next broker call fails
/// [`TokenError::Revoked`](orrery_policy::TokenError) rather than succeeding on
/// a grant nobody wants any more.
#[derive(Debug)]
pub struct LedgerRevoker(Arc<TokenLedger>);

impl LedgerRevoker {
    /// Revoke against the ledger the broker redeems from.
    #[must_use]
    pub fn new(ledger: Arc<TokenLedger>) -> Arc<Self> {
        Arc::new(Self(ledger))
    }
}

impl CallRevoker for LedgerRevoker {
    fn revoke(&self, call: CallId) {
        self.0.revoke_call(call);
    }
}
