//! The only door out of an extension.
//!
//! Everything an extension does to the outside world goes through this trait:
//! files, processes, the network, credentials. Nothing here hands out a raw
//! handle — no `File`, no `Child`, no socket — because a handle, once given,
//! cannot be budgeted, cancelled or audited afterwards.
//!
//! The implementation lives in `orrery-broker` (plan 07). This crate owns the
//! *facade*, so that a published extension can be written, compiled and tested
//! against it without depending on the harness.

use std::path::PathBuf;

use async_trait::async_trait;
use orrery_proto::{Aspect, CancelReason, Outcome, RuleId};

/// A refusal, a cancellation, or the harness breaking.
///
/// A **denial is not an error** anywhere the model can see it: it becomes
/// [`Outcome::Denied`] at the dispatch boundary. It is an `Err` here only
/// because an extension's own code needs `?` to stop early.
#[non_exhaustive]
#[derive(Clone, Debug, thiserror::Error)]
pub enum BrokerError {
    /// Policy said no, and named the rule that said it.
    #[error("denied: {reason}")]
    Denied {
        /// Which rule refused.
        rule: RuleId,
        /// Why, in words a person can act on.
        reason: String,
    },
    /// The call was stopped.
    #[error("cancelled: {reason:?}")]
    Cancelled {
        /// Why.
        reason: CancelReason,
    },
    /// This broker does not offer that.
    #[error("this broker does not offer {what}")]
    Unsupported {
        /// What was asked for.
        what: &'static str,
    },
    /// The operation was allowed and then went wrong.
    #[error("{message}")]
    Io {
        /// What went wrong.
        message: String,
    },
}

impl BrokerError {
    /// The refusal an extension's tool should return.
    ///
    /// The whole point of the type: a tool that does `Err(e) => e.into_outcome()`
    /// reports the refusal in the one shape every client already renders,
    /// instead of inventing a second vocabulary for "not allowed".
    #[must_use]
    pub fn into_outcome(self) -> Outcome {
        match self {
            BrokerError::Denied { rule, reason } => Outcome::Denied { rule, reason },
            BrokerError::Cancelled { reason } => Outcome::Cancelled { reason },
            BrokerError::Unsupported { what } => Outcome::Failed {
                code: "unsupported".to_owned(),
                message: format!("this broker does not offer {what}"),
            },
            BrokerError::Io { message } => Outcome::Failed {
                code: "io".to_owned(),
                message,
            },
        }
    }

    /// A denial that no numbered rule is responsible for.
    #[must_use]
    pub fn denied(reason: impl Into<String>) -> Self {
        BrokerError::Denied {
            rule: NO_RULE.parse().expect("the nil uuid is a uuid"),
            reason: reason.into(),
        }
    }

    /// A denial of a whole aspect, worded the way the broker words one.
    #[must_use]
    pub fn denied_aspect(aspect: Aspect, what: impl std::fmt::Display) -> Self {
        Self::denied(format!(
            "no `{aspect}` grant covers `{what}`",
            aspect = aspect_name(aspect)
        ))
    }
}

/// The rule id a refusal carries when no configured rule is responsible for it.
const NO_RULE: &str = "00000000-0000-0000-0000-000000000000";

/// An aspect's wire name, for error messages.
#[must_use]
pub fn aspect_name(aspect: Aspect) -> &'static str {
    match aspect {
        Aspect::Tool => "tool",
        Aspect::Mcp => "mcp",
        Aspect::Skill => "skill",
        Aspect::Ext => "ext",
        Aspect::Mode => "mode",
        Aspect::Read => "read",
        Aspect::Write => "write",
        Aspect::Spawn => "spawn",
        Aspect::Net => "net",
        Aspect::Creds => "creds",
        Aspect::Ui => "ui",
        Aspect::Render => "render",
        Aspect::MemRead => "mem.read",
        Aspect::MemWrite => "mem.write",
        _ => "unknown",
    }
}

/// What a broker call answers.
pub type BrokerResult<T> = Result<T, BrokerError>;

/// Read a bounded slice of a file.
///
/// There is no "read the whole file": `limit` is not optional, because §4.5's
/// objective-5 complaint is that a ceiling applied *after* the read has already
/// let the bytes into memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadRequest {
    /// Which file.
    pub path: PathBuf,
    /// Where to start.
    pub offset: u64,
    /// How much, at most.
    pub limit: u64,
}

impl ReadRequest {
    /// A read of at most `limit` bytes from the start.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, limit: u64) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            limit,
        }
    }

    /// Start somewhere other than the beginning.
    #[must_use]
    pub const fn at(mut self, offset: u64) -> Self {
        self.offset = offset;
        self
    }
}

/// What came back, and whether there is more.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadChunk {
    /// The bytes, never more than the request's `limit`.
    pub bytes: Vec<u8>,
    /// Whether the file ended inside this chunk.
    pub eof: bool,
    /// The file's total size, when the broker knows it.
    pub total: Option<u64>,
}

/// Replace a file's contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteRequest {
    /// Which file.
    pub path: PathBuf,
    /// The new contents.
    pub contents: Vec<u8>,
    /// Whether the write must be all-or-nothing: temp file, then rename, and
    /// reverted if the call is cancelled.
    pub atomic: bool,
}

impl WriteRequest {
    /// An atomic write.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) -> Self {
        Self {
            path: path.into(),
            contents: contents.into(),
            atomic: true,
        }
    }
}

/// Run a program and wait for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnRequest {
    /// The program.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Where to run it.
    pub cwd: Option<PathBuf>,
    /// How long to wait. `None` takes the call's own ceiling.
    pub timeout_ms: Option<u64>,
    /// How much output to keep. `None` takes the call's own ceiling.
    pub output_bytes: Option<u64>,
}

impl SpawnRequest {
    /// A spawn under the call's ceiling.
    #[must_use]
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            cwd: None,
            timeout_ms: None,
            output_bytes: None,
        }
    }

    /// Run it somewhere in particular.
    #[must_use]
    pub fn in_dir(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }
}

/// How a spawned program went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpawnOutput {
    /// Its exit code, when it had one.
    pub status: Option<i32>,
    /// What it wrote to stdout, up to the ceiling.
    pub stdout: Vec<u8>,
    /// What it wrote to stderr, up to the ceiling.
    pub stderr: Vec<u8>,
    /// Whether the ceiling cut it short.
    pub truncated: bool,
}

/// One HTTP request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetRequest {
    /// `GET`, `POST`, …
    pub method: String,
    /// Where to.
    pub url: String,
    /// Headers, in order.
    pub headers: Vec<(String, String)>,
    /// The body, if any.
    pub body: Option<Vec<u8>>,
}

/// One HTTP response.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetResponse {
    /// The status code.
    pub status: u16,
    /// Headers, in order.
    pub headers: Vec<(String, String)>,
    /// The body, up to the call's ceiling.
    pub body: Vec<u8>,
}

/// Everything an extension may reach, and nothing else.
///
/// Every method defaults to [`BrokerError::Unsupported`], so a broker that
/// offers three of the five is a legal broker rather than a compile error. What
/// it must never do is offer an operation it cannot police.
#[async_trait]
pub trait BrokerFacade: Send + Sync {
    /// Read at most `req.limit` bytes.
    async fn read(&self, req: ReadRequest) -> BrokerResult<ReadChunk> {
        let _ = req;
        Err(BrokerError::Unsupported { what: "fs reads" })
    }

    /// Write a file, atomically when asked.
    async fn write(&self, req: WriteRequest) -> BrokerResult<()> {
        let _ = req;
        Err(BrokerError::Unsupported { what: "fs writes" })
    }

    /// Run a program under the call's ceiling and containment.
    async fn spawn(&self, req: SpawnRequest) -> BrokerResult<SpawnOutput> {
        let _ = req;
        Err(BrokerError::Unsupported { what: "spawning" })
    }

    /// Make one HTTP request.
    async fn fetch(&self, req: NetRequest) -> BrokerResult<NetResponse> {
        let _ = req;
        Err(BrokerError::Unsupported {
            what: "the network",
        })
    }

    /// Read a named credential. The value never reaches the transcript.
    async fn credential(&self, name: &str) -> BrokerResult<String> {
        let _ = name;
        Err(BrokerError::Unsupported {
            what: "credentials",
        })
    }
}

/// The broker of a host that has not been given one.
///
/// Refuses everything, by name. Used as the default so that a host wired up
/// before `orrery-broker` exists denies rather than silently permits — the
/// direction a missing dependency has to fail in.
#[derive(Copy, Clone, Debug, Default)]
pub struct DeniesEverything;

#[async_trait]
impl BrokerFacade for DeniesEverything {
    async fn read(&self, req: ReadRequest) -> BrokerResult<ReadChunk> {
        Err(BrokerError::denied_aspect(Aspect::Read, req.path.display()))
    }

    async fn write(&self, req: WriteRequest) -> BrokerResult<()> {
        Err(BrokerError::denied_aspect(
            Aspect::Write,
            req.path.display(),
        ))
    }

    async fn spawn(&self, req: SpawnRequest) -> BrokerResult<SpawnOutput> {
        Err(BrokerError::denied_aspect(Aspect::Spawn, req.program))
    }

    async fn fetch(&self, req: NetRequest) -> BrokerResult<NetResponse> {
        Err(BrokerError::denied_aspect(Aspect::Net, req.url))
    }

    async fn credential(&self, name: &str) -> BrokerResult<String> {
        Err(BrokerError::denied_aspect(Aspect::Creds, name))
    }
}
