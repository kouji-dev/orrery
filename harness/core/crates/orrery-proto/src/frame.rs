//! The wire frames: what a client sends, what the kernel sends back, and the
//! one type that says how anything turned out.

use serde::{Deserialize, Serialize};

use crate::budget::Usage;
use crate::grant::Capability;
use crate::ids::{
    BranchId, CallId, ExtId, PromptId, ReqId, RuleId, Seq, SessionId, SurfaceId, TurnId,
};
use crate::message::ContentBlock;
use crate::scope::Subject;
use crate::surface::{Surface, SurfacePatch};

/// Why something stopped early.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CancelReason {
    /// Somebody asked.
    User,
    /// A budget ran out.
    Budget,
    /// A deadline passed.
    Timeout,
    /// The harness is going away.
    Shutdown,
    /// A parent was cancelled, so this was too.
    Parent,
}

/// How something turned out.
///
/// The type that makes "a denial is a value" real at every layer: the tool
/// registry returns it, the transcript stores it, the wire carries it, the
/// renderer draws it. Nothing anywhere raises a denial as an error, because an
/// error would have to be rendered by whoever caught it, and then two layers
/// would be describing the same refusal in two different ways.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum Outcome {
    /// It worked.
    Ok {
        /// What to show, when there is something to show.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        surface: Option<Surface>,
        /// What to feed back to the model, when that differs from what is
        /// shown.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        value: Option<serde_json::Value>,
    },
    /// Policy said no. The rule is named so the refusal can be explained and
    /// argued with, rather than being an opaque "not allowed".
    Denied {
        /// Which rule.
        rule: RuleId,
        /// Why, in words a person can act on.
        reason: String,
    },
    /// It worked, but produced more than it was allowed to emit.
    Truncated {
        /// What there is of it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        surface: Option<Surface>,
        /// How much was produced.
        bytes_emitted: u64,
        /// How much was allowed.
        limit: u64,
    },
    /// It was stopped.
    Cancelled {
        /// Why.
        reason: CancelReason,
    },
    /// The extension that owns it is no longer loaded.
    Unloaded {
        /// Which extension.
        ext: ExtId,
    },
    /// It ran and went wrong.
    Failed {
        /// A stable, machine-readable code.
        code: String,
        /// What went wrong, in words.
        message: String,
    },
}

impl Outcome {
    /// A bare success with nothing to show.
    #[must_use]
    pub fn ok() -> Self {
        Outcome::Ok {
            surface: None,
            value: None,
        }
    }

    /// A success that shows a surface.
    #[must_use]
    pub fn surface(surface: Surface) -> Self {
        Outcome::Ok {
            surface: Some(surface),
            value: None,
        }
    }

    /// Whether this outcome is a success, truncated or not.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        matches!(self, Outcome::Ok { .. } | Outcome::Truncated { .. })
    }
}

/// A tool's fully-qualified name: the extension that owns it, and the name
/// inside that extension.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolRef {
    /// The owning extension.
    pub ext: ExtId,
    /// The name inside it.
    pub name: String,
}

/// A string that is not a [`ToolRef`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("`{value}` is not a tool reference: expected `<ext>.<name>`")]
pub struct ToolRefError {
    /// The offending string.
    pub value: String,
}

impl std::fmt::Display for ToolRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.ext, self.name)
    }
}

impl std::str::FromStr for ToolRef {
    type Err = ToolRefError;

    /// Splits on the **last** dot, not the first.
    ///
    /// That is what makes `mcp.jira.create_issue` parse as the extension
    /// `mcp.jira` and the tool `create_issue`: an MCP server's namespace
    /// already contains a dot, so a first-dot split would put every MCP tool
    /// under an extension called `mcp`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || ToolRefError {
            value: s.to_owned(),
        };
        let (ext, name) = s.rsplit_once('.').ok_or_else(bad)?;
        if name.is_empty() {
            return Err(bad());
        }
        Ok(ToolRef {
            ext: ExtId::new(ext).map_err(|_| bad())?,
            name: name.to_owned(),
        })
    }
}

impl Serialize for ToolRef {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ToolRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error as _;
        let raw = String::deserialize(d)?;
        raw.parse().map_err(D::Error::custom)
    }
}

impl schemars::JsonSchema for ToolRef {
    fn schema_name() -> String {
        "ToolRef".to_owned()
    }

    fn json_schema(_: &mut schemars::r#gen::SchemaGenerator) -> schemars::schema::Schema {
        schemars::schema::SchemaObject {
            instance_type: Some(schemars::schema::InstanceType::String.into()),
            string: Some(Box::new(schemars::schema::StringValidation {
                pattern: Some("^(mcp\\.)?[a-z0-9-]+\\.[A-Za-z0-9_-]+$".to_owned()),
                ..Default::default()
            })),
            ..Default::default()
        }
        .into()
    }
}

/// What a person submitted.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UserInput {
    /// What they typed.
    pub text: String,
    /// Anything else they attached: images, a pasted tool result.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<ContentBlock>,
    /// Which branch of the turn tree to submit on. Absent means the current
    /// one; present means "re-run from here", which is how an edited turn forks
    /// rather than overwrites.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<BranchId>,
}

impl UserInput {
    /// Plain text on the current branch.
    #[must_use]
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

/// What is being asked for, and why.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ConsentPrompt {
    /// The handle an answer is matched to.
    pub id: PromptId,
    /// Who is asking.
    pub subject: Subject,
    /// What they want.
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    /// Why, in words a person can act on.
    pub reason: String,
    /// The rule that requires the ask, when one does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule: Option<RuleId>,
    /// Something to show alongside the question — the diff that is about to be
    /// written, the command that is about to run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface: Option<Surface>,
}

/// The answer to a [`ConsentPrompt`].
///
/// Four answers, not two: "yes" and "yes, and stop asking" are different
/// decisions, and so are "no" and "no, ever".
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ConsentAnswerKind {
    /// Allowed, this once.
    AllowOnce,
    /// Allowed, and remembered.
    AllowAlways,
    /// Refused, this once.
    Deny,
    /// Refused, and remembered.
    DenyAlways,
}

/// What a [`Request::Query`] is asking about.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum QueryOf {
    /// Every session.
    Sessions {},
    /// One session's state.
    Session {
        /// Which one.
        session: SessionId,
    },
    /// The tools visible in a session.
    Tools {
        /// Which session.
        session: SessionId,
    },
    /// What is loaded, and how it went — see [`crate::load::LoadOutcome`].
    Extensions {},
    /// The profiles a session can be created from.
    Profiles {},
}

/// How far an error reaches.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorScope {
    /// The connection. Nothing else will arrive.
    Transport,
    /// The session. Other sessions are fine.
    Session,
    /// This turn. The session survives.
    Turn,
    /// This call. The turn survives.
    Tool,
    /// One extension. Everything else survives.
    Ext,
}

/// What went wrong.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ErrorDetail {
    /// A stable, machine-readable code: `provider.unavailable`, `ext.crashed`.
    pub code: String,
    /// What went wrong, in words.
    pub message: String,
    /// Whether the same thing, tried again, might work.
    pub retryable: bool,
    /// Anything structured a client could use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Client to kernel.
///
/// Every variant carries an `id`, because every response has to name what it is
/// answering. It is repeated per variant rather than lifted to a struct field
/// because serde cannot express a struct-level field alongside an internally
/// tagged enum — a test asserts that no variant lost it.
///
/// The tags are **dotted** and spelled out one by one. No `rename_all` rule
/// produces a dot, so an added variant without an explicit `rename` would show
/// up as `turn-submit` instead of `turn.submit`; the tag test catches exactly
/// that.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "t")]
pub enum Request {
    /// Start a session.
    #[serde(rename = "session.create")]
    SessionCreate {
        /// The request id.
        id: ReqId,
        /// Which profile to build it from.
        profile: String,
        /// The workspace root.
        workspace: String,
    },
    /// Attach to an existing session, optionally replaying from a sequence
    /// number the client already has.
    #[serde(rename = "session.attach")]
    SessionAttach {
        /// The request id.
        id: ReqId,
        /// Which session.
        session: SessionId,
        /// Replay from just after this. Absent means from the start.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        since: Option<Seq>,
    },
    /// Submit a turn.
    #[serde(rename = "turn.submit")]
    TurnSubmit {
        /// The request id.
        id: ReqId,
        /// Which session.
        session: SessionId,
        /// What was submitted.
        input: UserInput,
    },
    /// Cancel a turn in flight.
    #[serde(rename = "turn.cancel")]
    TurnCancel {
        /// The request id.
        id: ReqId,
        /// Which session.
        session: SessionId,
        /// Which turn.
        turn: TurnId,
    },
    /// Answer a surface: a form submitted, a choice picked.
    #[serde(rename = "intent")]
    Intent {
        /// The request id.
        id: ReqId,
        /// Which session.
        session: SessionId,
        /// Which surface.
        surface: SurfaceId,
        /// What the surface produced.
        value: serde_json::Value,
    },
    /// Answer a consent prompt.
    #[serde(rename = "consent.answer")]
    ConsentAnswer {
        /// The request id.
        id: ReqId,
        /// Which prompt.
        prompt: PromptId,
        /// The answer.
        answer: ConsentAnswerKind,
    },
    /// Run a command.
    #[serde(rename = "command")]
    Command {
        /// The request id.
        id: ReqId,
        /// Which session.
        session: SessionId,
        /// The command's name.
        name: String,
        /// Its arguments.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<serde_json::Value>,
    },
    /// Ask about state.
    #[serde(rename = "query")]
    Query {
        /// The request id.
        id: ReqId,
        /// What is being asked about.
        of: QueryOf,
    },
}

impl Request {
    /// The id this request must be answered under.
    #[must_use]
    pub fn id(&self) -> ReqId {
        match self {
            Request::SessionCreate { id, .. }
            | Request::SessionAttach { id, .. }
            | Request::TurnSubmit { id, .. }
            | Request::TurnCancel { id, .. }
            | Request::Intent { id, .. }
            | Request::ConsentAnswer { id, .. }
            | Request::Command { id, .. }
            | Request::Query { id, .. } => *id,
        }
    }
}

/// Kernel to client.
///
/// Every variant carries a `seq`: a client detects a gap by arithmetic and
/// re-attaches with `since`, which is only possible if the ordering is on the
/// frame rather than implied by the transport.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "t")]
pub enum Event {
    /// A turn began.
    #[serde(rename = "turn.started")]
    TurnStarted {
        /// Where this sits in the session's order.
        seq: Seq,
        /// Which turn.
        turn: TurnId,
    },
    /// A surface changed.
    #[serde(rename = "delta")]
    Delta {
        /// Where this sits in the session's order.
        seq: Seq,
        /// Which surface.
        surface: SurfaceId,
        /// How it changed.
        patch: SurfacePatch,
    },
    /// A tool call began.
    #[serde(rename = "tool.started")]
    ToolStarted {
        /// Where this sits in the session's order.
        seq: Seq,
        /// Which call.
        call: CallId,
        /// Which tool.
        r#ref: ToolRef,
    },
    /// A tool call ended, one way or another.
    #[serde(rename = "tool.settled")]
    ToolSettled {
        /// Where this sits in the session's order.
        seq: Seq,
        /// Which call.
        call: CallId,
        /// How it went.
        outcome: Outcome,
    },
    /// Something needs permission.
    #[serde(rename = "consent.request")]
    ConsentRequest {
        /// Where this sits in the session's order.
        seq: Seq,
        /// The question.
        prompt: ConsentPrompt,
        /// How long there is to answer before the default applies.
        deadline_ms: u64,
    },
    /// A turn ended.
    #[serde(rename = "turn.settled")]
    TurnSettled {
        /// Where this sits in the session's order.
        seq: Seq,
        /// Which turn.
        turn: TurnId,
        /// What it cost.
        usage: Usage,
    },
    /// Something went wrong.
    #[serde(rename = "error")]
    Error {
        /// Where this sits in the session's order.
        seq: Seq,
        /// How far it reaches.
        scope: ErrorScope,
        /// What went wrong.
        detail: ErrorDetail,
    },
}

impl Event {
    /// Where this event sits in the session's order.
    #[must_use]
    pub fn seq(&self) -> Seq {
        match self {
            Event::TurnStarted { seq, .. }
            | Event::Delta { seq, .. }
            | Event::ToolStarted { seq, .. }
            | Event::ToolSettled { seq, .. }
            | Event::ConsentRequest { seq, .. }
            | Event::TurnSettled { seq, .. }
            | Event::Error { seq, .. } => *seq,
        }
    }
}
