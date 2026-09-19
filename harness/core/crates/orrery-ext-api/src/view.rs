//! Every loop event is a view (§6.7) — the vocabulary an extension contributes one in.
//!
//! # Why this is here and not in `orrery-surface`
//!
//! Contributing a view is an extension's job, exactly as contributing a tool
//! is: [`ViewBinding`] is to [`ToolDef`](crate::ToolDef) what a view is to a
//! tool. So the types an author needs in order to *contribute* — [`EventKind`],
//! [`LoopEvent`], [`Placement`], [`Predicate`], [`ViewBinding`] and the
//! [`ViewRegistry`] they check their bindings against — live in the published
//! crate, beside the mock broker. `orrery-surface` keeps what the kernel owns:
//! the differ, the hashes, the per-turn store, sealing and validation. It
//! re-exports these, so kernel-side code reads as it did.
//!
//! Making the loop legible is **not** inventing a surface per concept. The
//! catalogue already exists — the `Event` frames and the loop's own streams are
//! the full list of what happens in a turn — so this is binding what exists to
//! the primitives already defined.
//!
//! # Unbound is hidden, with one floor
//!
//! An event nobody bound does not render. That would make a fresh install show
//! nothing, so [`floor`] is the set of bindings a client is broken without:
//! assistant text, tool started and settled, consent and errors. Nothing in the
//! kernel installs them — `orrery-ext-views-default` does, through the
//! extension host like anything else, which is what keeps the mechanism honest
//! on the one case that most tempts a built-in shortcut.
//!
//! # The json renderer ignores placement
//!
//! Hiding is a human-client concern. [`ViewRegistry::render_json`] returns
//! every bound surface including the hidden ones, so a `--json` run and a TUI
//! run of the same turn are comparable — see the plan's open question 1.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use orrery_proto::{
    ConsentPrompt, ErrorDetail, ErrorScope, Event, Outcome, Status, Surface, SurfaceKind,
    TextStyle, ToolRef,
};
use serde::{Deserialize, Serialize};

/// What an event is called, in the dotted spelling a profile writes.
///
/// A plain name rather than an enum: an extension contributes its own loop
/// events, and a closed enum would mean the vocabulary grew a variant per
/// concept — exactly what §6.7 exists to avoid.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EventKind(String);

impl EventKind {
    /// Name an event kind.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The dotted name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Assistant prose. Part of the floor.
    pub const ASSISTANT_TEXT: &'static str = "assistant.text";
    /// A tool call began. Part of the floor.
    pub const TOOL_STARTED: &'static str = "tool.started";
    /// A tool call ended. Part of the floor.
    pub const TOOL_SETTLED: &'static str = "tool.settled";
    /// Something needs permission. Part of the floor.
    pub const CONSENT_REQUEST: &'static str = "consent.request";
    /// Something went wrong. Part of the floor.
    pub const ERROR: &'static str = "error";
    /// A turn began.
    pub const TURN_STARTED: &'static str = "turn.started";
    /// A turn ended.
    pub const TURN_SETTLED: &'static str = "turn.settled";
    /// A surface changed.
    pub const DELTA: &'static str = "delta";
    /// Where the provider's credentials stand. Part of the floor.
    ///
    /// The payload is `orrery_provider::AuthState`'s own serialisation — a
    /// `state` tag and camelCase fields. This crate does not depend on
    /// `orrery-provider` and must not: an extension author contributing a view
    /// sees the JSON, not the enum. The shape is pinned on both sides, here by
    /// [`floor`]'s binding and there by that crate's own test.
    pub const AUTH_STATE: &'static str = "auth.state";
}

impl From<&str> for EventKind {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl fmt::Display for EventKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One thing that happened in the loop, in the shape a binding renders.
///
/// [`LoopEvent::Frame`] is the protocol's own catalogue. The other two exist
/// because not everything the loop does is a wire frame: assistant prose
/// arrives as deltas, and an extension's own event is named and carries
/// whatever it carries.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum LoopEvent {
    /// Assistant prose, streaming or finished.
    AssistantText {
        /// Everything said so far.
        text: String,
        /// Whether the last delta has arrived.
        complete: bool,
    },
    /// One of the protocol's event frames.
    Frame(Box<Event>),
    /// Anything else in the loop, named the way a profile would write it.
    Other {
        /// Its dotted name.
        kind: EventKind,
        /// Whatever it carries.
        payload: serde_json::Value,
    },
}

impl LoopEvent {
    /// What this event is called.
    #[must_use]
    pub fn kind(&self) -> EventKind {
        match self {
            LoopEvent::AssistantText { .. } => EventKind::new(EventKind::ASSISTANT_TEXT),
            LoopEvent::Frame(frame) => EventKind::new(match frame.as_ref() {
                Event::TurnStarted { .. } => EventKind::TURN_STARTED,
                Event::Delta { .. } => EventKind::DELTA,
                Event::ToolStarted { .. } => EventKind::TOOL_STARTED,
                Event::ToolSettled { .. } => EventKind::TOOL_SETTLED,
                Event::ConsentRequest { .. } => EventKind::CONSENT_REQUEST,
                Event::TurnSettled { .. } => EventKind::TURN_SETTLED,
                Event::Error { .. } => EventKind::ERROR,
                // A frame this build does not know still has a name on the
                // wire; nothing binds it, so it is hidden, which is the rule.
                _ => "unknown",
            }),
            LoopEvent::Other { kind, .. } => kind.clone(),
        }
    }
}

/// Where a bound surface goes.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Placement {
    /// In the transcript, in order.
    #[default]
    Inline,
    /// In the client's status area, latest only.
    Footer,
    /// Not shown. The event still happened and is still in the audit.
    Hidden,
}

/// A condition on a binding: it fires only when this says so.
#[derive(Clone)]
pub struct Predicate(Arc<dyn Fn(&LoopEvent) -> bool + Send + Sync>);

impl Predicate {
    /// A predicate from any function.
    #[must_use]
    pub fn new(f: impl Fn(&LoopEvent) -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }

    /// Only when a tool settled badly — denied, failed or cancelled.
    #[must_use]
    pub fn outcome_is_not_ok() -> Self {
        Self::new(|event| match event {
            LoopEvent::Frame(frame) => match frame.as_ref() {
                Event::ToolSettled { outcome, .. } => !outcome.is_ok(),
                _ => false,
            },
            _ => false,
        })
    }

    /// Whether this event passes.
    #[must_use]
    pub fn matches(&self, event: &LoopEvent) -> bool {
        (self.0)(event)
    }
}

impl fmt::Debug for Predicate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Predicate(..)")
    }
}

/// One event kind, bound to one surface.
#[derive(Clone)]
pub struct ViewBinding {
    /// Which event.
    pub event: EventKind,
    /// When, if not always.
    pub when: Option<Predicate>,
    /// Where it goes. A profile overrides this without touching the code.
    pub placement: Placement,
    /// What it renders as. Code — which is to say, an extension.
    pub render: Arc<dyn Fn(&LoopEvent) -> Surface + Send + Sync>,
}

impl ViewBinding {
    /// Bind an event kind to a renderer, inline.
    #[must_use]
    pub fn new(
        event: impl Into<EventKind>,
        render: impl Fn(&LoopEvent) -> Surface + Send + Sync + 'static,
    ) -> Self {
        Self {
            event: event.into(),
            when: None,
            placement: Placement::Inline,
            render: Arc::new(render),
        }
    }

    /// Put it somewhere else.
    #[must_use]
    pub fn placed(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    /// Fire only when this holds.
    #[must_use]
    pub fn when(mut self, predicate: Predicate) -> Self {
        self.when = Some(predicate);
        self
    }

    /// Whether this binding fires for this event.
    #[must_use]
    pub fn fires(&self, event: &LoopEvent) -> bool {
        self.event == event.kind() && self.when.as_ref().is_none_or(|p| p.matches(event))
    }
}

impl fmt::Debug for ViewBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewBinding")
            .field("event", &self.event)
            .field("when", &self.when)
            .field("placement", &self.placement)
            .finish_non_exhaustive()
    }
}

/// A surface, and where the client was told to put it.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    /// Where it goes.
    pub placement: Placement,
    /// What to show.
    pub surface: Surface,
}

/// A profile that could not be read.
#[derive(Debug, thiserror::Error)]
#[error("a `[views]` table that is not one: {message}")]
pub struct ProfileError {
    /// What the TOML parser said.
    pub message: String,
}

#[derive(Debug, Default, Deserialize)]
struct Profile {
    #[serde(default)]
    views: BTreeMap<String, ViewEntry>,
}

#[derive(Debug, Default, Deserialize)]
struct ViewEntry {
    placement: Option<Placement>,
}

/// Every binding a session has, and the profile's opinion about where they go.
#[derive(Clone, Debug, Default)]
pub struct ViewRegistry {
    bindings: Vec<ViewBinding>,
    overrides: BTreeMap<EventKind, Placement>,
}

impl ViewRegistry {
    /// A registry with nothing bound. Every event is hidden until something
    /// binds it — that is the rule, and the floor is an extension's job.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a binding.
    pub fn bind(&mut self, binding: ViewBinding) {
        self.bindings.push(binding);
    }

    /// Add several.
    pub fn bind_all(&mut self, bindings: impl IntoIterator<Item = ViewBinding>) {
        self.bindings.extend(bindings);
    }

    /// What is bound, in registration order.
    #[must_use]
    pub fn bindings(&self) -> &[ViewBinding] {
        &self.bindings
    }

    /// Move or silence bindings from a profile's `[views.*]` table.
    ///
    /// ```toml
    /// [views."router.decided"]  placement = "inline"
    /// [views."tool.settled"]    placement = "footer"
    /// [views."skill.loaded"]    placement = "hidden"
    /// ```
    ///
    /// An entry for an event nothing binds is kept rather than refused: a
    /// profile is written once and extensions come and go, and a profile that
    /// failed to load because one extension is absent would be worse than a
    /// line that does nothing.
    ///
    /// # Errors
    ///
    /// [`ProfileError`] when the TOML is not this shape.
    pub fn apply_profile(&mut self, toml_src: &str) -> Result<(), ProfileError> {
        let profile: Profile = toml::from_str(toml_src).map_err(|e| ProfileError {
            message: e.message().to_owned(),
        })?;
        for (name, entry) in profile.views {
            if let Some(placement) = entry.placement {
                self.overrides.insert(EventKind::new(name), placement);
            }
        }
        Ok(())
    }

    /// Where this event's bindings were told to go, when a profile said.
    #[must_use]
    pub fn placement_of(&self, kind: &EventKind) -> Option<Placement> {
        self.overrides.get(kind).copied()
    }

    /// Render an event for a human client: what is bound, minus what is hidden.
    ///
    /// An unbound event produces nothing at all.
    #[must_use]
    pub fn render(&self, event: &LoopEvent) -> Vec<Placed> {
        self.render_all(event)
            .into_iter()
            .filter(|p| p.placement != Placement::Hidden)
            .collect()
    }

    /// Render an event for the json client: **placement is ignored**.
    ///
    /// Hiding is a human-client concern. A machine reading a run gets
    /// everything that was bound, so two runs stay comparable whatever the
    /// profile says.
    #[must_use]
    pub fn render_json(&self, event: &LoopEvent) -> Vec<Surface> {
        self.render_all(event)
            .into_iter()
            .map(|p| p.surface)
            .collect()
    }

    fn render_all(&self, event: &LoopEvent) -> Vec<Placed> {
        let kind = event.kind();
        let placement = self.overrides.get(&kind).copied();
        self.bindings
            .iter()
            .filter(|b| b.fires(event))
            .map(|b| Placed {
                placement: placement.unwrap_or(b.placement),
                surface: (b.render)(event),
            })
            .collect()
    }
}

/// The floor: the bindings a client is broken without.
///
/// Assistant text, tool started and settled, consent and errors. Nothing here
/// installs them — `orrery-ext-views-default` does, through the extension host
/// like any third-party bundle. This function is the *content* of that
/// extension, kept next to the vocabulary it is written in so the two cannot
/// drift; the loading path is the extension's, and there is no other.
#[must_use]
pub fn floor() -> Vec<ViewBinding> {
    vec![
        ViewBinding::new(EventKind::ASSISTANT_TEXT, |event| match event {
            LoopEvent::AssistantText { text, complete } => Surface {
                id: None,
                status: Some(if *complete {
                    Status::Done
                } else {
                    Status::Running
                }),
                kind: SurfaceKind::Markdown {
                    value: text.clone(),
                    complete: *complete,
                },
            },
            other => unexpected(other),
        }),
        ViewBinding::new(EventKind::TOOL_STARTED, |event| match event {
            LoopEvent::Frame(frame) => match frame.as_ref() {
                Event::ToolStarted { r#ref, .. } => tool_line(r#ref, Status::Running, None),
                other => unexpected(&LoopEvent::Frame(Box::new(other.clone()))),
            },
            other => unexpected(other),
        }),
        ViewBinding::new(EventKind::TOOL_SETTLED, |event| match event {
            LoopEvent::Frame(frame) => match frame.as_ref() {
                Event::ToolSettled { outcome, .. } => settled(outcome),
                other => unexpected(&LoopEvent::Frame(Box::new(other.clone()))),
            },
            other => unexpected(other),
        }),
        ViewBinding::new(EventKind::CONSENT_REQUEST, |event| match event {
            LoopEvent::Frame(frame) => match frame.as_ref() {
                Event::ConsentRequest {
                    prompt,
                    deadline_ms,
                    ..
                } => consent(prompt, *deadline_ms),
                other => unexpected(&LoopEvent::Frame(Box::new(other.clone()))),
            },
            other => unexpected(other),
        }),
        ViewBinding::new(EventKind::AUTH_STATE, |event| match event {
            LoopEvent::Other { payload, .. } => auth_state(payload),
            other => unexpected(other),
        }),
        ViewBinding::new(EventKind::ERROR, |event| match event {
            LoopEvent::Frame(frame) => match frame.as_ref() {
                Event::Error { scope, detail, .. } => error(*scope, detail),
                other => unexpected(&LoopEvent::Frame(Box::new(other.clone()))),
            },
            other => unexpected(other),
        }),
    ]
}

/// The event kinds the floor covers. A client showing none of these is broken
/// rather than minimal.
#[must_use]
pub fn floor_kinds() -> Vec<EventKind> {
    [
        EventKind::ASSISTANT_TEXT,
        EventKind::TOOL_STARTED,
        EventKind::TOOL_SETTLED,
        EventKind::CONSENT_REQUEST,
        EventKind::AUTH_STATE,
        EventKind::ERROR,
    ]
    .into_iter()
    .map(EventKind::new)
    .collect()
}

fn styled(value: impl Into<String>, style: TextStyle) -> Surface {
    Surface::new(SurfaceKind::Text {
        value: value.into(),
        style: Some(style),
    })
}

fn tool_line(r#ref: &ToolRef, status: Status, note: Option<&str>) -> Surface {
    let mut surface = styled(
        match note {
            Some(note) => format!("{} — {}", r#ref, note),
            None => r#ref.to_string(),
        },
        TextStyle::Muted,
    );
    surface.status = Some(status);
    surface
}

fn settled(outcome: &Outcome) -> Surface {
    match outcome {
        Outcome::Ok { surface, .. } => surface
            .clone()
            .unwrap_or_else(|| styled("done", TextStyle::Success)),
        Outcome::Truncated {
            surface,
            bytes_emitted,
            limit,
        } => {
            let note = styled(
                format!("truncated at {limit} bytes; it produced {bytes_emitted}"),
                TextStyle::Warning,
            );
            match surface {
                Some(surface) => Surface::new(SurfaceKind::Stack {
                    dir: orrery_proto::StackDir::Column,
                    title: None,
                    collapsed: false,
                    children: vec![surface.clone(), note],
                }),
                None => note,
            }
        }
        Outcome::Denied { reason, .. } => styled(reason.clone(), TextStyle::Warning),
        Outcome::Cancelled { .. } => styled("cancelled", TextStyle::Muted),
        Outcome::Unloaded { ext } => {
            styled(format!("`{ext}` is no longer loaded"), TextStyle::Warning)
        }
        Outcome::Failed { code, message } => styled(format!("{code}: {message}"), TextStyle::Error),
        // An outcome this build does not know still settles the call, and
        // saying so is better than saying nothing.
        _ => styled("settled", TextStyle::Muted),
    }
}

fn consent(prompt: &ConsentPrompt, deadline_ms: u64) -> Surface {
    Surface::new(SurfaceKind::Question {
        prompt: prompt.reason.clone(),
        choices: vec![
            orrery_proto::Choice {
                value: "allow-once".into(),
                label: "Allow once".into(),
            },
            orrery_proto::Choice {
                value: "allow-always".into(),
                label: "Allow always".into(),
            },
            orrery_proto::Choice {
                value: "deny".into(),
                label: "Deny".into(),
            },
            orrery_proto::Choice {
                value: "deny-always".into(),
                label: "Deny always".into(),
            },
        ],
        multi: false,
        free: false,
        default: Some("deny".into()),
        deadline_ms: Some(deadline_ms),
    })
}

/// A provider's credential state, drawn from the data rather than from prose.
///
/// The one that earns this binding is `pending`: a device-code login has a code
/// to type, a page to type it on, a deadline and a poll interval, and every one
/// of those is a field. A client that had to scrape them out of a sentence
/// would get a different answer per client, which is the failure §6.7 exists to
/// prevent.
///
/// An unrecognised `state` still renders — the tag, plainly — because a
/// renderer that draws nothing for a state it has not been taught is
/// indistinguishable from a harness that is simply stuck.
fn auth_state(payload: &serde_json::Value) -> Surface {
    let str_field = |name: &str| {
        payload
            .get(name)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    match payload.get("state").and_then(serde_json::Value::as_str) {
        Some("pending") => {
            let code = str_field("userCode");
            let uri = str_field("verificationUri");
            let complete = payload
                .get("verificationUriComplete")
                .and_then(serde_json::Value::as_str);
            let mut children = vec![
                styled(format!("Enter the code {code}"), TextStyle::Emphasis),
                styled(uri, TextStyle::Muted),
            ];
            if let Some(complete) = complete {
                children.push(styled(complete.to_owned(), TextStyle::Muted));
            }
            if let Some(interval) = payload.get("intervalSecs").and_then(serde_json::Value::as_u64)
            {
                // The interval the *server* stated, so a countdown a client
                // draws and the polling the flow does cannot drift.
                children.push(styled(
                    format!("checking every {interval}s"),
                    TextStyle::Muted,
                ));
            }
            let mut surface = Surface::new(SurfaceKind::Stack {
                dir: orrery_proto::StackDir::Column,
                title: Some("Sign in".to_owned()),
                collapsed: false,
                children,
            });
            surface.status = Some(Status::Running);
            surface
        }
        Some("needs-login") => {
            let mut surface = styled(str_field("reason"), TextStyle::Warning);
            surface.status = Some(Status::Failed);
            surface
        }
        Some("expired") => {
            let mut surface = styled("the credential has expired", TextStyle::Warning);
            surface.status = Some(Status::Failed);
            surface
        }
        Some("ready") => {
            let account = str_field("account");
            let mut surface = styled(
                if account.is_empty() {
                    "signed in".to_owned()
                } else {
                    format!("signed in as {account}")
                },
                TextStyle::Success,
            );
            surface.status = Some(Status::Done);
            surface
        }
        Some("anonymous") => styled("no credential needed", TextStyle::Muted),
        other => styled(
            format!(
                "auth state `{}`: this client has no renderer for it",
                other.unwrap_or("?")
            ),
            TextStyle::Muted,
        ),
    }
}

fn error(scope: ErrorScope, detail: &ErrorDetail) -> Surface {
    // How far it reaches is the first thing a reader needs: a tool error and a
    // session error look the same in a log and mean very different things.
    let reach = match scope {
        ErrorScope::Transport => "transport",
        ErrorScope::Session => "session",
        ErrorScope::Turn => "turn",
        ErrorScope::Tool => "tool",
        ErrorScope::Ext => "extension",
        _ => "unknown scope",
    };
    let mut surface = styled(
        format!(
            "{code}: {message} ({reach})",
            code = detail.code,
            message = detail.message
        ),
        TextStyle::Error,
    );
    surface.status = Some(Status::Failed);
    surface
}

/// A binding handed an event its kind says it cannot be handed.
///
/// Unreachable through [`ViewRegistry`], which only calls a binding whose kind
/// matches. It renders rather than panics because a renderer that can crash the
/// loop is worse than one that says something odd.
fn unexpected(event: &LoopEvent) -> Surface {
    styled(
        format!("{kind}: nothing to show", kind = event.kind()),
        TextStyle::Muted,
    )
}
