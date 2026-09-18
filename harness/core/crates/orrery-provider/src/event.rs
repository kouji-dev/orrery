//! What a streaming completion emits.
//!
//! Serde-representable on purpose: the fixture provider replays these straight
//! out of a `.jsonl` file, and a recorded stream that is not a literal
//! `ModelEvent` would drift from the type it is meant to pin.

use orrery_proto::{CallId, Usage};
use serde::{Deserialize, Serialize};

/// One thing the model did.
///
/// Tool-call arguments arrive as JSON **fragments**, because every streaming
/// API emits them that way. Reassembly happens once, in
/// [`ToolCallAccumulator`](crate::ToolCallAccumulator).
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum ModelEvent {
    /// The provider accepted the request and opened a response.
    Started {
        /// The provider's own id for this response, for correlation in a log.
        id: String,
    },
    /// A run of assistant prose.
    TextDelta {
        /// The text.
        text: String,
    },
    /// A run of exposed reasoning.
    ThinkingDelta {
        /// The text.
        text: String,
    },
    /// The model started asking for a tool.
    ToolUseStart {
        /// The call this opens.
        call: CallId,
        /// The fully-qualified tool name, as the model emitted it.
        name: String,
    },
    /// Part of the call's arguments. Not necessarily valid JSON on its own.
    ToolUseDelta {
        /// The call being filled in.
        call: CallId,
        /// A fragment of the argument JSON.
        json_fragment: String,
    },
    /// The call's arguments are complete.
    ToolUseEnd {
        /// The call this closes.
        call: CallId,
    },
    /// What it cost. May arrive more than once; the kernel sums.
    Usage {
        /// The spend so far.
        usage: Usage,
    },
    /// The response is over.
    Done {
        /// Why it stopped.
        stop: StopReason,
    },
}

/// Why a response ended.
///
/// A provider that reports a stop reason this enum does not know must fail
/// rather than pick a default: "the turn ended normally" is exactly the lie
/// that hides a truncation.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StopReason {
    /// The model finished its turn.
    EndTurn,
    /// The model wants a tool run and then to be called again.
    ToolUse,
    /// It hit `max_output_tokens`. The text is truncated.
    MaxTokens,
    /// It emitted one of the request's stop sequences.
    StopSequence,
    /// The model declined.
    Refusal,
}
