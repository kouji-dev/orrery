//! The vendored AG-UI event enum, and the frame that carries our `seq`.
//!
//! Vendored rather than depended on: there is no first-party Rust SDK, and we
//! only ever *produce* AG-UI, so a client crate would be dead weight. The
//! variant names are pinned in [`crate::drift`] and diffed against upstream's
//! published schema by `cargo xtask agui-drift`.

use serde::{Deserialize, Serialize};

/// One RFC 6902 operation, plus the one operation RFC 6902 cannot express.
///
/// `append` is ours. JSON Patch has no way to say "add these six characters to
/// the end of that string", and the whole point of [`SurfacePatch::Append`] is
/// that the hot path is a string and not a tree — re-sending the body once per
/// token would undo that. Every other op here is stock.
///
/// [`SurfacePatch::Append`]: orrery_proto::SurfacePatch::Append
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "kebab-case")]
pub enum PatchOp {
    /// Stock RFC 6902 `replace`.
    Replace {
        /// A JSON Pointer.
        path: String,
        /// What it becomes.
        value: serde_json::Value,
    },
    /// Stock RFC 6902 `add`.
    Add {
        /// A JSON Pointer.
        path: String,
        /// What to add.
        value: serde_json::Value,
    },
    /// Stock RFC 6902 `remove`.
    Remove {
        /// A JSON Pointer.
        path: String,
    },
    /// **Ours.** Concatenate `value` onto the string at `path`.
    Append {
        /// A JSON Pointer to a string.
        path: String,
        /// What to concatenate.
        value: String,
    },
}

impl PatchOp {
    /// The pointer this op addresses.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            PatchOp::Replace { path, .. }
            | PatchOp::Add { path, .. }
            | PatchOp::Remove { path }
            | PatchOp::Append { path, .. } => path,
        }
    }
}

/// An AG-UI event.
///
/// Internally tagged on `type` with `SCREAMING_SNAKE_CASE` names and camelCase
/// fields, which is what upstream puts on the wire.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AguiEvent {
    /// A run began. Ours: one turn.
    #[serde(rename = "RUN_STARTED", rename_all = "camelCase")]
    RunStarted {
        /// The session.
        thread_id: String,
        /// The turn.
        run_id: String,
    },
    /// A run ended well.
    #[serde(rename = "RUN_FINISHED", rename_all = "camelCase")]
    RunFinished {
        /// The session.
        thread_id: String,
        /// The turn.
        run_id: String,
        /// Ours: the turn's [`Usage`](orrery_proto::Usage).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        result: Option<serde_json::Value>,
    },
    /// A run ended badly.
    #[serde(rename = "RUN_ERROR", rename_all = "camelCase")]
    RunError {
        /// What went wrong, in words.
        message: String,
        /// A stable, machine-readable code.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    /// A step began (§4.6's mode changes).
    #[serde(rename = "STEP_STARTED", rename_all = "camelCase")]
    StepStarted {
        /// Which step.
        step_name: String,
    },
    /// A step ended.
    #[serde(rename = "STEP_FINISHED", rename_all = "camelCase")]
    StepFinished {
        /// Which step.
        step_name: String,
    },
    /// An assistant message opened.
    #[serde(rename = "TEXT_MESSAGE_START", rename_all = "camelCase")]
    TextMessageStart {
        /// The message's id. Ours: the markdown surface's id.
        message_id: String,
        /// Always `assistant` for us.
        role: String,
    },
    /// A chunk of that message.
    #[serde(rename = "TEXT_MESSAGE_CONTENT", rename_all = "camelCase")]
    TextMessageContent {
        /// Which message.
        message_id: String,
        /// The chunk.
        delta: String,
    },
    /// That message is complete.
    #[serde(rename = "TEXT_MESSAGE_END", rename_all = "camelCase")]
    TextMessageEnd {
        /// Which message.
        message_id: String,
    },
    /// A tool call opened.
    #[serde(rename = "TOOL_CALL_START", rename_all = "camelCase")]
    ToolCallStart {
        /// Which call.
        tool_call_id: String,
        /// The fully-qualified tool name.
        tool_call_name: String,
        /// The message the call hangs off, when there is one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_message_id: Option<String>,
    },
    /// A fragment of that call's arguments.
    #[serde(rename = "TOOL_CALL_ARGS", rename_all = "camelCase")]
    ToolCallArgs {
        /// Which call.
        tool_call_id: String,
        /// The fragment, as it came off the model.
        delta: String,
    },
    /// The arguments are complete.
    #[serde(rename = "TOOL_CALL_END", rename_all = "camelCase")]
    ToolCallEnd {
        /// Which call.
        tool_call_id: String,
    },
    /// How the call turned out.
    #[serde(rename = "TOOL_CALL_RESULT", rename_all = "camelCase")]
    ToolCallResult {
        /// The result message's id.
        message_id: String,
        /// Which call.
        tool_call_id: String,
        /// The result, as text. Ours also carries `surface`.
        content: String,
        /// Ours: the [`Outcome`](orrery_proto::Outcome), verbatim.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outcome: Option<serde_json::Value>,
    },
    /// The whole shared state.
    #[serde(rename = "STATE_SNAPSHOT", rename_all = "camelCase")]
    StateSnapshot {
        /// The state.
        snapshot: serde_json::Value,
    },
    /// A change to the shared state, as JSON Patch.
    #[serde(rename = "STATE_DELTA", rename_all = "camelCase")]
    StateDelta {
        /// The ops, in order.
        delta: Vec<PatchOp>,
    },
    /// Anything AG-UI has no vocabulary for: consent, above all.
    #[serde(rename = "CUSTOM", rename_all = "camelCase")]
    Custom {
        /// A namespaced name: `orrery.consent.request`.
        name: String,
        /// Whatever that name means.
        value: serde_json::Value,
    },
}

impl AguiEvent {
    /// The `type` tag this event serialises under.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            AguiEvent::RunStarted { .. } => "RUN_STARTED",
            AguiEvent::RunFinished { .. } => "RUN_FINISHED",
            AguiEvent::RunError { .. } => "RUN_ERROR",
            AguiEvent::StepStarted { .. } => "STEP_STARTED",
            AguiEvent::StepFinished { .. } => "STEP_FINISHED",
            AguiEvent::TextMessageStart { .. } => "TEXT_MESSAGE_START",
            AguiEvent::TextMessageContent { .. } => "TEXT_MESSAGE_CONTENT",
            AguiEvent::TextMessageEnd { .. } => "TEXT_MESSAGE_END",
            AguiEvent::ToolCallStart { .. } => "TOOL_CALL_START",
            AguiEvent::ToolCallArgs { .. } => "TOOL_CALL_ARGS",
            AguiEvent::ToolCallEnd { .. } => "TOOL_CALL_END",
            AguiEvent::ToolCallResult { .. } => "TOOL_CALL_RESULT",
            AguiEvent::StateSnapshot { .. } => "STATE_SNAPSHOT",
            AguiEvent::StateDelta { .. } => "STATE_DELTA",
            AguiEvent::Custom { .. } => "CUSTOM",
        }
    }
}

/// An AG-UI event with our `seq` on it.
///
/// `seq` is assigned once, per session, at the differ's output — never per
/// connection — so it belongs on the frame rather than being implied by the
/// order a particular socket happened to deliver things in. AG-UI's own events
/// have no such field; it flattens alongside them, which a stock client ignores
/// and ours uses to detect a gap by arithmetic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// Where this sits in the session's order.
    pub seq: u64,
    /// The event.
    #[serde(flatten)]
    pub event: AguiEvent,
}

impl Frame {
    /// Stamp an event with a sequence number.
    #[must_use]
    pub fn new(seq: u64, event: AguiEvent) -> Self {
        Self { seq, event }
    }
}
