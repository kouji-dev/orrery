//! Reassembling tool-call arguments — once, for every provider.

use std::collections::HashMap;

use orrery_proto::CallId;

use crate::event::ModelEvent;

/// A tool call whose arguments are complete and parsed.
#[derive(Clone, Debug, PartialEq)]
pub struct CompletedToolCall {
    /// The call.
    pub call: CallId,
    /// The tool the model named.
    pub name: String,
    /// The parsed arguments. Not yet validated against the tool's schema —
    /// that is the registry's job at dispatch.
    pub input: serde_json::Value,
}

/// A stream that did not add up.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum AccumulateError {
    /// The fragments concatenated into something that is not JSON. A model
    /// mistake or a truncated stream — never a panic.
    #[error("tool call `{name}` ({call}): arguments are not valid JSON: {message}; raw: {raw}")]
    MalformedJson {
        /// The call.
        call: CallId,
        /// The tool named at the start of the call.
        name: String,
        /// What was accumulated, so the failure is diagnosable.
        raw: String,
        /// The parser's complaint.
        message: String,
    },
    /// A delta or an end arrived for a call that never started.
    #[error("tool call {call}: {what} without a start")]
    UnknownCall {
        /// The call.
        call: CallId,
        /// Which event arrived out of order.
        what: &'static str,
    },
}

#[derive(Debug)]
struct Partial {
    name: String,
    json: String,
}

/// Accumulates `ToolUse{Start,Delta,End}` into complete calls.
///
/// One implementation for all providers: not three times in three provider
/// crates and not a fourth time in the kernel.
#[derive(Debug, Default)]
pub struct ToolCallAccumulator {
    open: HashMap<CallId, Partial>,
}

impl ToolCallAccumulator {
    /// An accumulator with nothing in flight.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// True while at least one call is still streaming.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }

    /// Feed one event.
    ///
    /// Returns `Some` exactly once per call, at its `ToolUseEnd` — or at an
    /// out-of-order event, which is reported rather than swallowed. Every other
    /// event yields `None`.
    ///
    /// The plan sketches this as `-> Option<CompletedToolCall>`; it returns a
    /// `Result` inside the `Option` because the plan also requires that
    /// malformed JSON be an error and not a panic, and there is nowhere else
    /// for that error to go.
    pub fn feed(&mut self, ev: &ModelEvent) -> Option<Result<CompletedToolCall, AccumulateError>> {
        match ev {
            ModelEvent::ToolUseStart { call, name } => {
                self.open.insert(
                    *call,
                    Partial {
                        name: name.clone(),
                        json: String::new(),
                    },
                );
                None
            }
            ModelEvent::ToolUseDelta {
                call,
                json_fragment,
            } => match self.open.get_mut(call) {
                Some(p) => {
                    p.json.push_str(json_fragment);
                    None
                }
                None => Some(Err(AccumulateError::UnknownCall {
                    call: *call,
                    what: "delta",
                })),
            },
            ModelEvent::ToolUseEnd { call } => {
                let Some(p) = self.open.remove(call) else {
                    return Some(Err(AccumulateError::UnknownCall {
                        call: *call,
                        what: "end",
                    }));
                };
                // A tool with no arguments streams no fragments at all. That is
                // an empty object, not a parse failure.
                let raw = if p.json.trim().is_empty() {
                    "{}"
                } else {
                    p.json.as_str()
                };
                Some(match serde_json::from_str(raw) {
                    Ok(input) => Ok(CompletedToolCall {
                        call: *call,
                        name: p.name,
                        input,
                    }),
                    Err(e) => Err(AccumulateError::MalformedJson {
                        call: *call,
                        name: p.name,
                        raw: p.json,
                        message: e.to_string(),
                    }),
                })
            }
            _ => None,
        }
    }
}
