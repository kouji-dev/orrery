//! `SurfaceStore`: the projection every renderer draws from.
//!
//! This is where text deltas become a markdown surface and tool calls become a
//! tool stack, so **no renderer reimplements that**. A ratatui widget, an Ink
//! component and a `<div>` all consume the same [`TurnView`]; the only thing
//! they disagree about is pixels.
//!
//! The store is a projection, never an authority. It is rebuilt from the event
//! stream and it is never written to by the client: an edit a person makes
//! travels the other way, as an `intent` the kernel validates.

use orrery_agui::{AguiEvent, Frame, PatchOp};
use orrery_proto::{Outcome, StackDir, Status, Surface, SurfaceKind, TextStyle};
use serde::{Deserialize, Serialize};

/// The turn surfaces land in when replay picks up mid-turn.
///
/// A client that re-attached with `since` never saw the `RUN_STARTED` that
/// opened the turn it is now in the middle of. Those surfaces are still real,
/// so they go here rather than on the floor.
pub const DETACHED_TURN: &str = "(detached)";

/// A hole in the sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    /// The `seq` that should have come next.
    pub expected: u64,
    /// The `seq` that did.
    pub got: u64,
}

/// One surface, as a client holds it.
///
/// The id is an opaque string rather than a [`SurfaceId`](orrery_proto::SurfaceId)
/// because on the AG-UI wire it is a message id or a tool-call id, which a
/// stock client assigns and which is not required to be a uuid.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SurfaceView {
    /// The handle a patch addresses it by.
    pub id: String,
    /// How far along it is.
    pub status: Option<Status>,
    /// What it is.
    pub kind: SurfaceKind,
}

/// A consent prompt, and how it stands.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PromptView {
    /// The handle an answer is matched to.
    pub id: String,
    /// Why, in words a person can act on.
    pub reason: String,
    /// How long there is to answer.
    pub deadline_ms: u64,
    /// `None` while it is still a question.
    pub resolved: Option<Resolution>,
}

/// How a prompt was settled.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Resolution {
    /// What was decided.
    pub answer: orrery_proto::ConsentAnswerKind,
    /// `user` or `fallback`.
    pub by: String,
}

/// What went wrong in a turn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TurnError {
    /// In words.
    pub message: String,
    /// A stable code, when there is one.
    pub code: Option<String>,
}

/// One turn: everything the kernel did about one input.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TurnView {
    /// The run id.
    pub id: String,
    /// Whether it is over.
    pub settled: bool,
    /// Whether it was stopped rather than finished.
    pub cancelled: bool,
    /// What went wrong, if anything.
    pub error: Option<TurnError>,
    /// In insertion order, not sorted: a table that arrived before a diff is
    /// drawn before it.
    pub surfaces: Vec<SurfaceView>,
    /// What it stopped to ask.
    pub prompts: Vec<PromptView>,
}

impl TurnView {
    fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            settled: false,
            cancelled: false,
            error: None,
            surfaces: Vec::new(),
            prompts: Vec::new(),
        }
    }

    fn surface_mut(&mut self, id: &str) -> Option<&mut SurfaceView> {
        self.surfaces.iter_mut().find(|s| s.id == id)
    }

    /// One surface by id.
    #[must_use]
    pub fn surface(&self, id: &str) -> Option<&SurfaceView> {
        self.surfaces.iter().find(|s| s.id == id)
    }
}

/// The whole store, in the shape the conformance fixtures assert.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StoreState {
    /// The last `seq` applied.
    pub last_seq: Option<u64>,
    /// The turn the live region shows.
    pub live: Option<String>,
    /// Every hole detected so far.
    pub gaps: Vec<Gap>,
    /// In arrival order.
    pub turns: Vec<TurnView>,
}

/// What changed, so a renderer can redraw a part rather than everything.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum StoreChange {
    /// A turn opened.
    TurnStarted {
        /// Which.
        turn: String,
    },
    /// A turn closed.
    TurnSettled {
        /// Which.
        turn: String,
    },
    /// A surface was created or changed.
    SurfaceChanged {
        /// Which turn it is in.
        turn: String,
        /// Which surface.
        surface: String,
    },
    /// A surface went away.
    SurfaceRemoved {
        /// Which turn it was in.
        turn: String,
        /// Which surface.
        surface: String,
    },
    /// Something is being asked.
    PromptRaised {
        /// Which prompt.
        prompt: String,
    },
    /// It was answered, one way or another.
    PromptResolved {
        /// Which prompt.
        prompt: String,
    },
    /// A frame was lost.
    ///
    /// A renderer's cue to re-attach with `since = <the last seq it saw>`. The
    /// event that revealed the gap is still applied: a hole is a reason to ask
    /// for the missing frames, not to throw the turn away.
    GapDetected {
        /// The `seq` that should have come next.
        expected: u64,
        /// The `seq` that did.
        got: u64,
    },
    /// An event this client has no handler for. Ignored, not rejected.
    Ignored {
        /// Its `type`, or its `Custom` name.
        what: String,
    },
}

/// The projection itself.
#[derive(Clone, Debug, Default)]
pub struct SurfaceStore {
    state: StoreState,
}

impl SurfaceStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The whole state, as the conformance fixtures spell it.
    #[must_use]
    pub fn state(&self) -> &StoreState {
        &self.state
    }

    /// One turn by run id.
    #[must_use]
    pub fn turn(&self, id: &str) -> Option<&TurnView> {
        self.state.turns.iter().find(|t| t.id == id)
    }

    /// Every settled turn, oldest first. What scrollback shows.
    pub fn settled(&self) -> impl Iterator<Item = &TurnView> {
        self.state.turns.iter().filter(|t| t.settled)
    }

    /// The turn in flight. What the live region shows.
    #[must_use]
    pub fn live(&self) -> Option<&TurnView> {
        self.state
            .live
            .as_ref()
            .and_then(|id| self.state.turns.iter().find(|t| &t.id == id))
    }

    /// Every gap detected so far.
    #[must_use]
    pub fn gaps(&self) -> &[Gap] {
        &self.state.gaps
    }

    /// Apply one frame.
    ///
    /// Never rejects: an event type or a `Custom` name this client has never
    /// heard of is reported as [`StoreChange::Ignored`] and still consumes its
    /// sequence number, because a client that refused unknown events would
    /// break the moment the kernel learned a new one.
    pub fn apply(&mut self, frame: &Frame) -> Vec<StoreChange> {
        let mut changes = self.note(frame.first_seq(), frame.seq);
        changes.extend(self.apply_event(&frame.event));
        changes
    }

    /// Account for a frame this client could not even parse.
    ///
    /// An event `type` from a newer kernel still occupies a sequence number, so
    /// skipping it silently would turn every forward-compatible addition into a
    /// phantom gap.
    pub fn apply_unknown(&mut self, seq: u64, what: &str) -> Vec<StoreChange> {
        let mut changes = self.note(seq, seq);
        changes.push(StoreChange::Ignored {
            what: what.to_owned(),
        });
        changes
    }

    fn note(&mut self, first: u64, seq: u64) -> Vec<StoreChange> {
        let mut changes = Vec::new();
        if let Some(prev) = self.state.last_seq
            && first != prev + 1
        {
            let gap = Gap {
                expected: prev + 1,
                got: first,
            };
            self.state.gaps.push(gap);
            changes.push(StoreChange::GapDetected {
                expected: gap.expected,
                got: gap.got,
            });
        }
        self.state.last_seq = Some(seq);
        changes
    }

    fn apply_event(&mut self, event: &AguiEvent) -> Vec<StoreChange> {
        match event {
            AguiEvent::RunStarted { run_id, .. } => {
                self.state.turns.push(TurnView::new(run_id));
                self.state.live = Some(run_id.clone());
                vec![StoreChange::TurnStarted {
                    turn: run_id.clone(),
                }]
            }
            AguiEvent::RunFinished { .. } => self.settle(None),
            AguiEvent::RunError { message, code } => self.settle(Some(TurnError {
                message: message.clone(),
                code: code.clone(),
            })),
            AguiEvent::TextMessageStart { message_id, .. } => self.upsert(message_id, |s| {
                s.status = Some(Status::Running);
                s.kind = SurfaceKind::Markdown {
                    value: String::new(),
                    complete: false,
                };
            }),
            AguiEvent::TextMessageContent { message_id, delta } => self.upsert(message_id, |s| {
                if let SurfaceKind::Markdown { value, .. } = &mut s.kind {
                    value.push_str(delta);
                } else {
                    s.kind = SurfaceKind::Markdown {
                        value: delta.clone(),
                        complete: false,
                    };
                    s.status = Some(Status::Running);
                }
            }),
            AguiEvent::TextMessageEnd { message_id } => self.upsert(message_id, |s| {
                if let SurfaceKind::Markdown { complete, .. } = &mut s.kind {
                    *complete = true;
                }
                s.status = Some(Status::Done);
            }),
            AguiEvent::ToolCallStart {
                tool_call_id,
                tool_call_name,
                ..
            } => self.upsert(tool_call_id, |s| {
                s.status = Some(Status::Running);
                s.kind = SurfaceKind::Stack {
                    dir: StackDir::Column,
                    title: Some(tool_call_name.clone()),
                    collapsed: false,
                    children: vec![Surface::new(SurfaceKind::Text {
                        value: String::new(),
                        style: Some(TextStyle::Code),
                    })],
                };
            }),
            AguiEvent::ToolCallArgs {
                tool_call_id,
                delta,
            } => self.upsert(tool_call_id, |s| {
                if let SurfaceKind::Stack { children, .. } = &mut s.kind
                    && let Some(SurfaceKind::Text { value, .. }) =
                        children.first_mut().map(|c| &mut c.kind)
                {
                    value.push_str(delta);
                }
            }),
            // The arguments are complete; the call is not. Nothing to draw yet.
            AguiEvent::ToolCallEnd { tool_call_id } => vec![StoreChange::Ignored {
                what: format!("TOOL_CALL_END:{tool_call_id}"),
            }],
            AguiEvent::ToolCallResult {
                tool_call_id,
                content,
                outcome,
                ..
            } => {
                let parsed: Option<Outcome> =
                    outcome.clone().and_then(|v| serde_json::from_value(v).ok());
                let status = parsed.as_ref().map_or(Status::Done, outcome_status);
                let result = parsed
                    .as_ref()
                    .and_then(outcome_surface)
                    .unwrap_or_else(|| {
                        Surface::new(SurfaceKind::Text {
                            value: content.clone(),
                            style: None,
                        })
                    });
                self.upsert(tool_call_id, move |s| {
                    s.status = Some(status);
                    if let SurfaceKind::Stack { children, .. } = &mut s.kind {
                        children.push(result.clone());
                    }
                })
            }
            AguiEvent::StateSnapshot { snapshot } => self.snapshot(snapshot),
            AguiEvent::StateDelta { delta } => delta.iter().flat_map(|op| self.patch(op)).collect(),
            AguiEvent::Custom { name, value } => self.custom(name, value),
            other => vec![StoreChange::Ignored {
                what: other.type_name().to_owned(),
            }],
        }
    }

    /// The turn new surfaces land in.
    fn target(&mut self) -> usize {
        if let Some(live) = &self.state.live
            && let Some(at) = self.state.turns.iter().position(|t| &t.id == live)
        {
            return at;
        }
        if let Some(at) = self.state.turns.iter().rposition(|t| !t.settled) {
            return at;
        }
        self.state.turns.push(TurnView::new(DETACHED_TURN));
        self.state.turns.len() - 1
    }

    fn upsert(&mut self, id: &str, edit: impl FnOnce(&mut SurfaceView)) -> Vec<StoreChange> {
        let at = self.target();
        let turn = &mut self.state.turns[at];
        if turn.surface_mut(id).is_none() {
            turn.surfaces.push(SurfaceView {
                id: id.to_owned(),
                status: None,
                kind: SurfaceKind::Text {
                    value: String::new(),
                    style: None,
                },
            });
        }
        let surface = turn.surface_mut(id).expect("just inserted");
        edit(surface);
        vec![StoreChange::SurfaceChanged {
            turn: turn.id.clone(),
            surface: id.to_owned(),
        }]
    }

    fn settle(&mut self, error: Option<TurnError>) -> Vec<StoreChange> {
        let at = self.target();
        let turn = &mut self.state.turns[at];
        turn.settled = true;
        turn.error = error;
        // Close anything still open. A turn that stopped closes its message as
        // cancelled, never as done: a half-answer must not read as an answer.
        let closing = if turn.cancelled {
            Status::Cancelled
        } else {
            Status::Done
        };
        for surface in &mut turn.surfaces {
            if surface.status == Some(Status::Running) {
                if let SurfaceKind::Markdown { complete, .. } = &mut surface.kind {
                    *complete = true;
                }
                surface.status = Some(closing);
            }
        }
        let id = turn.id.clone();
        self.state.live = None;
        vec![StoreChange::TurnSettled { turn: id }]
    }

    fn snapshot(&mut self, snapshot: &serde_json::Value) -> Vec<StoreChange> {
        let at = self.target();
        let mut changes = Vec::new();
        let Some(surfaces) = snapshot.get("surfaces").and_then(|v| v.as_object()) else {
            return vec![StoreChange::Ignored {
                what: "STATE_SNAPSHOT".into(),
            }];
        };
        self.state.turns[at].surfaces.clear();
        for (id, value) in surfaces {
            if let Some(view) = surface_from_json(id, value) {
                self.state.turns[at].surfaces.push(view);
                changes.push(StoreChange::SurfaceChanged {
                    turn: self.state.turns[at].id.clone(),
                    surface: id.clone(),
                });
            }
        }
        changes
    }

    fn patch(&mut self, op: &PatchOp) -> Vec<StoreChange> {
        let Some((id, rest)) = split_pointer(op.path()) else {
            return vec![StoreChange::Ignored {
                what: format!("patch outside /surfaces: {}", op.path()),
            }];
        };
        let at = self.target();
        let turn_id = self.state.turns[at].id.clone();

        if rest.is_empty() {
            return match op {
                // A `replace` at the surface root creates as well as swaps: the
                // kernel sends one op whether or not this client has seen the
                // surface before, because it does not track what each client
                // knows.
                PatchOp::Replace { value, .. } | PatchOp::Add { value, .. } => {
                    let Some(view) = surface_from_json(&id, value) else {
                        return vec![StoreChange::Ignored {
                            what: format!("unreadable surface at {}", op.path()),
                        }];
                    };
                    let turn = &mut self.state.turns[at];
                    match turn.surfaces.iter_mut().find(|s| s.id == id) {
                        Some(existing) => *existing = view,
                        None => turn.surfaces.push(view),
                    }
                    vec![StoreChange::SurfaceChanged {
                        turn: turn_id,
                        surface: id,
                    }]
                }
                PatchOp::Remove { .. } => {
                    self.state.turns[at].surfaces.retain(|s| s.id != id);
                    vec![StoreChange::SurfaceRemoved {
                        turn: turn_id,
                        surface: id,
                    }]
                }
                PatchOp::Append { .. } => vec![StoreChange::Ignored {
                    what: "append to a whole surface".into(),
                }],
            };
        }

        // A write inside a surface. Round-tripping through JSON keeps one
        // implementation of the pointer rules rather than a match arm per field.
        let Some(surface) = self.state.turns[at].surface_mut(&id) else {
            return vec![StoreChange::Ignored {
                what: format!("patch to an unknown surface {id}"),
            }];
        };
        let Ok(mut json) = serde_json::to_value(&*surface) else {
            return vec![StoreChange::Ignored {
                what: format!("unserialisable surface {id}"),
            }];
        };
        let applied = match op {
            PatchOp::Replace { value, .. } | PatchOp::Add { value, .. } => {
                json.pointer_mut(&rest).map(|slot| *slot = value.clone())
            }
            PatchOp::Append { value, .. } => json.pointer_mut(&rest).map(|slot| {
                if let serde_json::Value::String(s) = slot {
                    s.push_str(value);
                } else {
                    *slot = serde_json::Value::String(value.clone());
                }
            }),
            PatchOp::Remove { .. } => json.pointer_mut(&rest).map(|slot| {
                *slot = serde_json::Value::Null;
            }),
        };
        if applied.is_none() {
            return vec![StoreChange::Ignored {
                what: format!("no such field {}", op.path()),
            }];
        }
        match serde_json::from_value::<SurfaceView>(json) {
            Ok(updated) => {
                *surface = updated;
                vec![StoreChange::SurfaceChanged {
                    turn: turn_id,
                    surface: id,
                }]
            }
            Err(_) => vec![StoreChange::Ignored {
                what: format!("patch left surface {id} ill-formed"),
            }],
        }
    }

    fn custom(&mut self, name: &str, value: &serde_json::Value) -> Vec<StoreChange> {
        match name {
            orrery_agui::CONSENT_REQUEST => {
                let Some(prompt) = prompt_from_json(value) else {
                    return vec![StoreChange::Ignored {
                        what: name.to_owned(),
                    }];
                };
                let id = prompt.id.clone();
                let at = self.target();
                self.state.turns[at].prompts.push(prompt);
                vec![StoreChange::PromptRaised { prompt: id }]
            }
            orrery_agui::CONSENT_RESOLVED => {
                let id = value
                    .get("prompt_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let resolution = Resolution {
                    answer: serde_json::from_value(
                        value
                            .get("answer")
                            .cloned()
                            .unwrap_or(serde_json::Value::Null),
                    )
                    .unwrap_or(orrery_proto::ConsentAnswerKind::Deny),
                    by: value
                        .get("by")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("fallback")
                        .to_owned(),
                };
                let existing = self
                    .state
                    .turns
                    .iter_mut()
                    .flat_map(|t| t.prompts.iter_mut())
                    .find(|p| p.id == id);
                match existing {
                    Some(prompt) => prompt.resolved = Some(resolution),
                    None => {
                        // Replayed as already-resolved: the client never saw the
                        // question, only the answer.
                        let mut prompt = prompt_from_json(value).unwrap_or_else(|| PromptView {
                            id: id.clone(),
                            reason: String::new(),
                            deadline_ms: 0,
                            resolved: None,
                        });
                        prompt.resolved = Some(resolution);
                        let at = self.target();
                        self.state.turns[at].prompts.push(prompt);
                    }
                }
                vec![StoreChange::PromptResolved { prompt: id }]
            }
            orrery_agui::TURN_CANCELLED => {
                let at = self.target();
                self.state.turns[at].cancelled = true;
                vec![StoreChange::TurnSettled {
                    turn: self.state.turns[at].id.clone(),
                }]
            }
            other => vec![StoreChange::Ignored {
                what: other.to_owned(),
            }],
        }
    }
}

/// Split `/surfaces/<id>/rest` into the id and the pointer below it.
fn split_pointer(path: &str) -> Option<(String, String)> {
    let rest = path.strip_prefix("/surfaces/")?;
    let (id, below) = match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, ""),
    };
    if id.is_empty() {
        return None;
    }
    Some((unescape(id), below.to_owned()))
}

/// RFC 6901 §4, in reverse.
fn unescape(token: &str) -> String {
    token.replace("~1", "/").replace("~0", "~")
}

fn surface_from_json(id: &str, value: &serde_json::Value) -> Option<SurfaceView> {
    let mut value = value.clone();
    if let Some(obj) = value.as_object_mut() {
        obj.insert("id".into(), serde_json::Value::String(id.to_owned()));
        obj.entry("status").or_insert(serde_json::Value::Null);
    }
    serde_json::from_value(value).ok()
}

fn prompt_from_json(value: &serde_json::Value) -> Option<PromptView> {
    let prompt = value.get("prompt")?;
    Some(PromptView {
        id: prompt.get("id")?.as_str()?.to_owned(),
        reason: prompt
            .get("reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        deadline_ms: value
            .get("deadline_ms")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        resolved: None,
    })
}

fn outcome_status(outcome: &Outcome) -> Status {
    match outcome {
        Outcome::Ok { .. } | Outcome::Truncated { .. } => Status::Done,
        Outcome::Cancelled { .. } => Status::Cancelled,
        _ => Status::Failed,
    }
}

fn outcome_surface(outcome: &Outcome) -> Option<Surface> {
    match outcome {
        Outcome::Ok { surface, .. } | Outcome::Truncated { surface, .. } => surface.clone(),
        _ => None,
    }
}
