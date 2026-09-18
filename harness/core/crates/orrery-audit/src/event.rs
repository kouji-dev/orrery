//! The events. One append-only stream, with redaction built into the shapes.

use std::collections::BTreeMap;

use orrery_proto::{CallId, ExtId, Layer, PromptId, RuleId, Subject};
use serde::{Deserialize, Serialize};

use crate::redact::{ContentRef, Digest};

/// What a call did, as far as the audit is concerned.
///
/// Deliberately coarse. The detail belongs to the session store; the audit
/// answers "did it run, and did it work".
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallOutcome {
    /// It ran and succeeded.
    Ok,
    /// It ran and failed.
    Failed,
    /// Policy refused it.
    Denied,
    /// It was cancelled before it finished.
    Cancelled,
}

/// The verdict a capability decision reached.
///
/// Mirrors `orrery-policy`'s `Decision` without depending on it: audit is the
/// bottom of the stack and nothing above it may become a cycle.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// Allowed, and a token was minted.
    Allow,
    /// The user has to be asked.
    Ask,
    /// Refused.
    Deny,
}

/// One line of the audit stream.
///
/// Every tagged variant names its wire tag explicitly: several are dotted and
/// no `rename_all` rule produces a dot, so a derived name would be a silently
/// different schema.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t")]
pub enum AuditEvent {
    /// An extension was loaded, degraded, skipped or failed.
    #[serde(rename = "ext.load")]
    ExtensionLoad {
        /// Which extension.
        ext: ExtId,
        /// `ok`, `degraded`, `skipped` or `failed`.
        status: String,
        /// What it contributed, by name.
        #[serde(default)]
        contributions: Vec<String>,
        /// What is missing, when something is.
        #[serde(default)]
        problems: Vec<String>,
    },
    /// A capability decision, **with the rule that produced it**.
    ///
    /// This is the event the whole crate exists for: "which rule allowed this"
    /// has to be answerable from the stream alone.
    #[serde(rename = "capability.decision")]
    CapabilityDecision {
        /// Who asked.
        subject: Subject,
        /// What they asked for, in rule-grammar form — `write(./src/main.rs)`.
        request: String,
        /// What was decided.
        verdict: Verdict,
        /// The rule that decided it, when a rule did.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rule: Option<RuleId>,
        /// The rule as written, so the stream is readable without the config.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rule_text: Option<String>,
        /// Which layer the rule came from.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        layer: Option<Layer>,
        /// Why, in words.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    /// A tool call, with its input hashed rather than kept.
    #[serde(rename = "tool.call")]
    ToolCall {
        /// Which call.
        call: CallId,
        /// The fully-qualified tool name.
        tool: String,
        /// The digest of the input. The input itself is never here.
        input: Digest,
        /// How it went.
        outcome: CallOutcome,
    },
    /// The registry resolved a name that could have meant more than one thing.
    ///
    /// Ambiguity is logged, never fatal.
    #[serde(rename = "tool.name")]
    ToolName {
        /// The name as called.
        name: String,
        /// Everything it could have meant.
        candidates: Vec<String>,
        /// What it was taken to mean.
        chose: String,
    },
    /// A model request, with its token counts.
    #[serde(rename = "model.request")]
    ModelRequest {
        /// Which model.
        model: String,
        /// Tokens in.
        input_tokens: u64,
        /// Tokens out.
        output_tokens: u64,
    },
    /// A consent prompt was answered.
    #[serde(rename = "consent.answer")]
    ConsentAnswer {
        /// Which prompt.
        prompt: PromptId,
        /// `allow-once`, `allow-always`, `deny`, `deny-always`, or `timeout`.
        answer: String,
    },
    /// A sub-agent was spawned.
    #[serde(rename = "agent.spawn")]
    SubAgentSpawn {
        /// Who spawned it.
        parent: Subject,
        /// What it is called.
        agent: String,
    },
    /// A routing decision, with the signal values that produced it.
    #[serde(rename = "route.decision")]
    RoutingDecision {
        /// What was chosen.
        chose: String,
        /// The signals and their values.
        #[serde(default)]
        signals: BTreeMap<String, f64>,
    },
    /// Something recorded by reference: a memory entry, a surface payload, a
    /// prompt body. Never the content itself.
    #[serde(rename = "content.ref")]
    Content {
        /// What happened to it — `mem.write`, `surface.push`, `prompt.render`.
        action: String,
        /// Where to find it, and what it hashed to.
        content: ContentRef,
    },
}

impl AuditEvent {
    /// A tool-call event whose input is hashed on the way in.
    ///
    /// The only constructor that takes a raw input, and it does not keep it.
    #[must_use]
    pub fn tool_call(
        call: CallId,
        tool: impl Into<String>,
        input: &serde_json::Value,
        outcome: CallOutcome,
    ) -> Self {
        AuditEvent::ToolCall {
            call,
            tool: tool.into(),
            input: Digest::of_json(input),
            outcome,
        }
    }

    /// Which of the three streams this event belongs to.
    #[must_use]
    pub fn stream(&self) -> crate::layer::Stream {
        use crate::layer::Stream;
        match self {
            AuditEvent::ExtensionLoad { .. } => Stream::Load,
            AuditEvent::ModelRequest { .. } | AuditEvent::RoutingDecision { .. } => {
                Stream::Telemetry
            }
            _ => Stream::Audit,
        }
    }
}

/// An event as it sits in the stream: ordered, timestamped, immutable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AuditRecord {
    /// Position in this sink's stream, from zero, with no gaps.
    pub seq: u64,
    /// Wall clock, milliseconds since the epoch.
    pub at_ms: u64,
    /// What happened.
    #[serde(flatten)]
    pub event: AuditEvent,
}

impl AuditRecord {
    pub(crate) fn new(seq: u64, event: AuditEvent) -> Self {
        Self {
            seq,
            at_ms: now_ms(),
            event,
        }
    }
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
