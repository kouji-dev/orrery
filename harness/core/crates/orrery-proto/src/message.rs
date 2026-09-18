//! The provider-neutral conversation.
//!
//! Deliberately small. Anything one provider wants and another has never heard
//! of is built in the provider crate, from these; carrying it here would make
//! every consumer of the protocol learn one vendor's vocabulary.

use serde::{Deserialize, Serialize};

use crate::frame::Outcome;
use crate::ids::CallId;

/// Who said it.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MessageRole {
    /// The harness, setting the frame.
    System,
    /// The person, or a tool result being handed back.
    User,
    /// The model.
    Assistant,
}

/// One piece of a message.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "t", rename_all = "kebab-case")]
pub enum ContentBlock {
    /// Prose.
    Text {
        /// The text.
        text: String,
    },
    /// An image, inline and base64. A provider without an `images` capability
    /// rejects a message carrying one rather than dropping it silently.
    Image {
        /// The IANA media type, e.g. `image/png`.
        media_type: String,
        /// Base64, without a data-URL prefix.
        data: String,
    },
    /// The model asking for a tool.
    ToolUse {
        /// The call this block opens.
        call: CallId,
        /// The fully-qualified tool name, `<ext>.<name>`.
        name: String,
        /// The arguments, already validated against the tool's schema.
        input: serde_json::Value,
    },
    /// What happened.
    ///
    /// Carries the same [`Outcome`] the `tool.settled` frame carries — one
    /// shape for "what happened", whether it is being shown to a person or fed
    /// back to a model. That is what makes a denial identical in the transcript
    /// and on the wire, instead of a rendered apology in one and a structured
    /// value in the other.
    ToolResult {
        /// The call this block closes.
        call: CallId,
        /// How it went.
        outcome: Outcome,
    },
    /// Reasoning the provider exposed.
    ///
    /// Kept rather than dropped: every current provider emits it, the
    /// transcript is the audit record, and a turn tree that silently discards
    /// what the model was thinking cannot be replayed faithfully.
    Thinking {
        /// The reasoning text.
        text: String,
    },
}

/// One turn of the conversation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Message {
    /// Who said it.
    pub role: MessageRole,
    /// What they said.
    #[serde(default)]
    pub content: Vec<ContentBlock>,
}

impl Message {
    /// A message that is one run of text.
    #[must_use]
    pub fn text(role: MessageRole, text: impl Into<String>) -> Self {
        Self {
            role,
            content: vec![ContentBlock::Text { text: text.into() }],
        }
    }
}
