//! Kernel frames in, AG-UI events out. One direction, on purpose.

use std::collections::HashSet;

use orrery_proto::{Event, Outcome, Surface, SurfaceId, SurfaceKind, SurfacePatch};

use crate::event::{AguiEvent, PatchOp};
use crate::map;

/// The encoder, and the little state the mapping needs.
///
/// One per **session**, never one per connection: it sits at the differ's
/// output, upstream of every listener, which is the same place `seq` is
/// assigned. Two clients therefore see the same events under the same numbers,
/// and a re-encode of the session store from the start reproduces them exactly.
///
/// The state is two sets of ids.
///
/// **Open messages** exist because AG-UI splits what our protocol unifies: a
/// markdown surface being streamed is `TextMessageStart` / `Content` / `End`
/// to an AG-UI client and one patched surface to us, and only the encoder knows
/// which of the two a given `Append` belongs to.
///
/// **Open calls** exist because our frames have no "tool arguments" event and
/// AG-UI does. A call streams into the surface that shows it, whose id **is**
/// the call's id, so a `delta` on that id while the call is open is that call's
/// arguments — `TOOL_CALL_ARGS` — and the same delta after it settles is an
/// ordinary `StateDelta` on an ordinary surface. Without this a client sees
/// `TOOL_CALL_START` then `TOOL_CALL_END` and never learns what the tool was
/// asked to do.
#[derive(Clone, Debug)]
pub struct Encoder {
    thread_id: String,
    open_messages: HashSet<SurfaceId>,
    open_calls: HashSet<SurfaceId>,
}

impl Encoder {
    /// A fresh encoder for one session.
    #[must_use]
    pub fn new(thread_id: impl Into<String>) -> Self {
        Self {
            thread_id: thread_id.into(),
            open_messages: HashSet::new(),
            open_calls: HashSet::new(),
        }
    }

    /// The session this encoder speaks for.
    #[must_use]
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }

    /// Map one kernel frame to the AG-UI events it produces.
    ///
    /// Never empty: every [`Event`] variant produces at least one event, so
    /// nothing crosses the differ and then vanishes on the wire.
    pub fn encode(&mut self, frame: &Event) -> Vec<AguiEvent> {
        match frame {
            Event::TurnStarted { turn, .. } => vec![AguiEvent::RunStarted {
                thread_id: self.thread_id.clone(),
                run_id: turn.to_string(),
            }],
            Event::TurnSettled { turn, usage, .. } => {
                // Any message still open at the end of a turn is closed, so a
                // client never holds an unterminated message across turns.
                let mut out: Vec<AguiEvent> = self
                    .open_messages
                    .drain()
                    .map(|id| AguiEvent::TextMessageEnd {
                        message_id: id.to_string(),
                    })
                    .collect();
                out.sort_by_key(AguiEvent::type_name);
                out.push(AguiEvent::RunFinished {
                    thread_id: self.thread_id.clone(),
                    run_id: turn.to_string(),
                    result: serde_json::to_value(usage).ok(),
                });
                out
            }
            Event::Delta { patch, .. } => self.encode_patch(patch),
            Event::ToolStarted { call, r#ref, .. } => {
                self.open_calls.insert(call_surface(*call));
                vec![AguiEvent::ToolCallStart {
                    tool_call_id: call.to_string(),
                    tool_call_name: r#ref.to_string(),
                    parent_message_id: None,
                }]
            }
            Event::ToolSettled { call, outcome, .. } => {
                self.open_calls.remove(&call_surface(*call));
                vec![
                AguiEvent::ToolCallEnd {
                    tool_call_id: call.to_string(),
                },
                AguiEvent::ToolCallResult {
                    message_id: format!("{call}:result"),
                    tool_call_id: call.to_string(),
                    content: outcome_text(outcome),
                    outcome: serde_json::to_value(outcome).ok(),
                },
            ]
            }
            Event::ConsentRequest {
                prompt,
                deadline_ms,
                ..
            } => vec![AguiEvent::Custom {
                name: map::CONSENT_REQUEST.to_owned(),
                value: serde_json::json!({
                    "prompt": prompt,
                    "deadline_ms": deadline_ms,
                }),
            }],
            Event::Error { scope, detail, .. } => vec![AguiEvent::RunError {
                message: detail.message.clone(),
                code: Some(format!(
                    "{}:{}",
                    serde_json::to_value(scope)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_else(|| "turn".into()),
                    detail.code
                )),
            }],
            // `Event` is `#[non_exhaustive]`: an unmapped variant becomes a
            // `Custom` rather than nothing, so it is visible instead of lost.
            other => vec![AguiEvent::Custom {
                name: "orrery.unmapped".to_owned(),
                value: serde_json::to_value(other).unwrap_or(serde_json::Value::Null),
            }],
        }
    }

    /// Close any message this encoder still holds open.
    ///
    /// Called when a connection's session ends without a `turn.settled`.
    pub fn close_open_messages(&mut self) -> Vec<AguiEvent> {
        let mut ids: Vec<SurfaceId> = self.open_messages.drain().collect();
        ids.sort();
        ids.into_iter()
            .map(|id| AguiEvent::TextMessageEnd {
                message_id: id.to_string(),
            })
            .collect()
    }

    fn encode_patch(&mut self, patch: &SurfacePatch) -> Vec<AguiEvent> {
        match patch {
            SurfacePatch::Replace { id, value } => {
                // A call whose arguments arrive in one piece rather than as
                // fragments: a non-streaming provider, or a replay that has the
                // parsed input already.
                if self.open_calls.contains(id)
                    && let Some(text) = code_body(value)
                {
                    return vec![AguiEvent::ToolCallArgs {
                        tool_call_id: id.to_string(),
                        delta: text.to_owned(),
                    }];
                }
                if is_message(value) && !self.open_messages.contains(id) {
                    self.open_messages.insert(*id);
                    let mut out = vec![AguiEvent::TextMessageStart {
                        message_id: id.to_string(),
                        role: "assistant".to_owned(),
                    }];
                    if let Some(text) = message_body(value)
                        && !text.is_empty()
                    {
                        out.push(AguiEvent::TextMessageContent {
                            message_id: id.to_string(),
                            delta: text.to_owned(),
                        });
                    }
                    if message_complete(value) {
                        self.open_messages.remove(id);
                        out.push(AguiEvent::TextMessageEnd {
                            message_id: id.to_string(),
                        });
                    }
                    out
                } else {
                    vec![state_delta(PatchOp::Replace {
                        path: map::surface_pointer(*id),
                        value: serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
                    })]
                }
            }
            SurfacePatch::Append { id, text } if self.open_calls.contains(id) => {
                vec![AguiEvent::ToolCallArgs {
                    tool_call_id: id.to_string(),
                    delta: text.clone(),
                }]
            }
            SurfacePatch::Append { id, text } => {
                if self.open_messages.contains(id) {
                    vec![AguiEvent::TextMessageContent {
                        message_id: id.to_string(),
                        delta: text.clone(),
                    }]
                } else {
                    // The one op RFC 6902 cannot express. See `PatchOp::Append`.
                    vec![state_delta(PatchOp::Append {
                        path: map::field_pointer(*id, &["kind".to_owned(), "value".to_owned()]),
                        value: text.clone(),
                    })]
                }
            }
            SurfacePatch::Set { id, path, value } => {
                let closes_message = self.open_messages.contains(id)
                    && path.last().is_some_and(|last| last == "complete")
                    && value == &serde_json::Value::Bool(true);
                if closes_message {
                    self.open_messages.remove(id);
                    vec![AguiEvent::TextMessageEnd {
                        message_id: id.to_string(),
                    }]
                } else {
                    vec![state_delta(PatchOp::Replace {
                        path: map::field_pointer(*id, path),
                        value: value.clone(),
                    })]
                }
            }
            SurfacePatch::Remove { id } => {
                let mut out = Vec::new();
                if self.open_messages.remove(id) {
                    out.push(AguiEvent::TextMessageEnd {
                        message_id: id.to_string(),
                    });
                }
                out.push(state_delta(PatchOp::Remove {
                    path: map::surface_pointer(*id),
                }));
                out
            }
            // `SurfacePatch` is `#[non_exhaustive]`.
            other => vec![AguiEvent::Custom {
                name: "orrery.unmapped".to_owned(),
                value: serde_json::to_value(other).unwrap_or(serde_json::Value::Null),
            }],
        }
    }
}

fn state_delta(op: PatchOp) -> AguiEvent {
    AguiEvent::StateDelta { delta: vec![op] }
}

/// A call id, as the id of the surface that shows the call.
///
/// The two are the same uuid on purpose: it is what lets a `delta` name a call
/// without a second frame variant. See the note on [`Encoder`].
fn call_surface(call: orrery_proto::CallId) -> SurfaceId {
    SurfaceId::from_uuid(*call.as_uuid())
}

/// The text of a surface that carries a call's arguments verbatim.
fn code_body(s: &Surface) -> Option<&str> {
    match &s.kind {
        SurfaceKind::Text { value, .. } => Some(value),
        SurfaceKind::Markdown { value, .. } => Some(value),
        _ => None,
    }
}

/// Whether a surface is the kind AG-UI calls a text message.
fn is_message(s: &Surface) -> bool {
    matches!(s.kind, SurfaceKind::Markdown { .. })
}

fn message_body(s: &Surface) -> Option<&str> {
    match &s.kind {
        SurfaceKind::Markdown { value, .. } => Some(value),
        _ => None,
    }
}

fn message_complete(s: &Surface) -> bool {
    matches!(s.kind, SurfaceKind::Markdown { complete: true, .. })
}

/// What a tool result reads as when a client has nowhere to put a surface.
fn outcome_text(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Ok { value: Some(v), .. } => v.to_string(),
        Outcome::Ok { .. } => "ok".to_owned(),
        Outcome::Denied { rule, reason } => format!("denied by {rule}: {reason}"),
        Outcome::Truncated {
            bytes_emitted,
            limit,
            ..
        } => format!("truncated: {bytes_emitted} bytes emitted, limit {limit}"),
        Outcome::Cancelled { reason } => format!("cancelled: {reason:?}"),
        Outcome::Unloaded { ext } => format!("extension {ext} is no longer loaded"),
        Outcome::Failed { code, message } => format!("{code}: {message}"),
        other => format!("{other:?}"),
    }
}
