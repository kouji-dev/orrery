//! View bindings: unbound is hidden, the floor is always there, and a profile
//! moves things without touching code.

use orrery_proto::{
    Capability, ConsentPrompt, ErrorDetail, ErrorScope, Event, ExtId, Outcome, PromptId, Seq,
    Subject, Surface, SurfaceKind, TextStyle, ToolRef, TurnId, Usage,
};
use orrery_surface::{
    EventKind, LoopEvent, Placement, Predicate, SurfaceStore, ViewBinding, ViewRegistry, floor,
    floor_kinds,
};

fn text(value: &str) -> Surface {
    Surface::new(SurfaceKind::Text {
        value: value.to_owned(),
        style: None,
    })
}

fn tool_started() -> LoopEvent {
    LoopEvent::Frame(Box::new(Event::ToolStarted {
        seq: Seq(2),
        call: orrery_proto::CallId::new(),
        r#ref: "builtin.read".parse::<ToolRef>().expect("a tool ref"),
    }))
}

fn tool_settled(outcome: Outcome) -> LoopEvent {
    LoopEvent::Frame(Box::new(Event::ToolSettled {
        seq: Seq(3),
        call: orrery_proto::CallId::new(),
        outcome,
    }))
}

fn consent_request() -> LoopEvent {
    LoopEvent::Frame(Box::new(Event::ConsentRequest {
        seq: Seq(4),
        prompt: ConsentPrompt {
            id: PromptId::new(),
            subject: Subject::Ext(ExtId::new("builtin").expect("an ext id")),
            capabilities: Vec::<Capability>::new(),
            reason: "write to src/lib.rs".into(),
            rule: None,
            surface: None,
        },
        deadline_ms: 30_000,
    }))
}

fn error_event() -> LoopEvent {
    LoopEvent::Frame(Box::new(Event::Error {
        seq: Seq(5),
        scope: ErrorScope::Tool,
        detail: ErrorDetail {
            code: "provider.unavailable".into(),
            message: "the provider did not answer".into(),
            retryable: true,
            data: None,
        },
    }))
}

fn floor_registry() -> ViewRegistry {
    let mut registry = ViewRegistry::new();
    registry.bind_all(floor());
    registry
}

/// An event nobody bound produces no surface at all.
#[test]
fn unbound_is_hidden() {
    let registry = ViewRegistry::new();
    assert!(registry.render(&tool_started()).is_empty());
    assert!(
        registry.render_json(&tool_started()).is_empty(),
        "unbound is unbound in every client, including json"
    );

    // Binding something else does not bind this.
    let registry = floor_registry();
    let unheard = LoopEvent::Other {
        kind: EventKind::new("skill.loaded"),
        payload: serde_json::json!({ "skill": "review" }),
    };
    assert!(registry.render(&unheard).is_empty());
    assert!(
        registry
            .render(&LoopEvent::Frame(Box::new(Event::TurnSettled {
                seq: Seq(9),
                turn: TurnId::new(),
                usage: Usage::default(),
            })))
            .is_empty(),
        "turn.settled is not in the floor, so nothing shows it until something binds it"
    );
}

/// With zero configuration, assistant text, tool started and settled, consent
/// and errors all render. A client showing nothing until configured is broken
/// rather than minimal.
#[test]
fn floor_is_always_bound() {
    let registry = floor_registry();

    let events = [
        LoopEvent::AssistantText {
            text: "the workspace has three crates".into(),
            complete: true,
        },
        tool_started(),
        tool_settled(Outcome::ok()),
        consent_request(),
        error_event(),
    ];
    assert_eq!(
        events.len(),
        floor_kinds().len(),
        "one event per floor kind, and no more"
    );

    for event in &events {
        let placed = registry.render(event);
        assert_eq!(
            placed.len(),
            1,
            "{} renders with no configuration",
            event.kind()
        );
        assert_eq!(placed[0].placement, Placement::Inline);
        assert!(
            floor_kinds().contains(&event.kind()),
            "{} is part of the floor",
            event.kind()
        );
    }

    // And what they render is the right shape, not merely non-empty.
    let assistant = registry.render(&events[0]);
    assert!(matches!(
        assistant[0].surface.kind,
        SurfaceKind::Markdown { complete: true, .. }
    ));
    let consent = registry.render(&events[3]);
    let SurfaceKind::Question {
        prompt, choices, ..
    } = &consent[0].surface.kind
    else {
        panic!("consent is a question, not prose");
    };
    assert_eq!(prompt, "write to src/lib.rs");
    assert_eq!(choices.len(), 4, "four answers, not two");
    let failure = registry.render(&error_event());
    let SurfaceKind::Text { style, value } = &failure[0].surface.kind else {
        panic!("an error is text");
    };
    assert_eq!(*style, Some(TextStyle::Error));
    assert!(value.contains("provider.unavailable"));
}

/// A `[views.*]` table moves a binding to the footer without touching code.
#[test]
fn placement_from_profile() {
    let mut registry = floor_registry();
    assert_eq!(
        registry.render(&tool_started())[0].placement,
        Placement::Inline
    );

    registry
        .apply_profile(
            r#"
            [views."tool.started"]
            placement = "footer"

            [views."tool.settled"]
            placement = "hidden"

            # An event nothing binds. Kept, and does nothing.
            [views."router.decided"]
            placement = "inline"
            "#,
        )
        .expect("the profile loads");

    assert_eq!(
        registry.render(&tool_started())[0].placement,
        Placement::Footer,
        "moved by the profile"
    );
    assert!(
        registry.render(&tool_settled(Outcome::ok())).is_empty(),
        "hidden is not rendered for a human client"
    );
    assert_eq!(
        registry.placement_of(&EventKind::new("router.decided")),
        Some(Placement::Inline),
        "an override for an unbound event is kept, not refused"
    );
}

/// The json renderer ignores `placement` entirely: hiding is a human-client
/// concern, and two runs must stay comparable whatever the profile says.
#[test]
fn json_ignores_placement() {
    let mut registry = floor_registry();
    registry
        .apply_profile(
            r#"
            [views."tool.settled"]
            placement = "hidden"
            "#,
        )
        .expect("the profile loads");

    let event = tool_settled(Outcome::ok());
    assert!(registry.render(&event).is_empty(), "hidden for a person");
    assert_eq!(
        registry.render_json(&event).len(),
        1,
        "and still there for a machine"
    );
}

/// A binding with `when` fires only on matching events.
#[test]
fn when_predicate() {
    let mut registry = ViewRegistry::new();
    registry.bind(
        ViewBinding::new(EventKind::TOOL_SETTLED, |_| text("that went wrong"))
            .when(Predicate::outcome_is_not_ok())
            .placed(Placement::Footer),
    );

    assert!(
        registry.render(&tool_settled(Outcome::ok())).is_empty(),
        "a success does not fire an error-only binding"
    );

    let failed = registry.render(&tool_settled(Outcome::Failed {
        code: "fs.denied".into(),
        message: "no".into(),
    }));
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].placement, Placement::Footer);

    // And a predicate can be anything, including something about a payload.
    let mut registry = ViewRegistry::new();
    registry.bind(
        ViewBinding::new("skill.loaded", |_| text("a skill loaded")).when(Predicate::new(
            |event| match event {
                LoopEvent::Other { payload, .. } => payload.get("skill").is_some(),
                _ => false,
            },
        )),
    );
    assert!(
        registry
            .render(&LoopEvent::Other {
                kind: EventKind::new("skill.loaded"),
                payload: serde_json::json!({}),
            })
            .is_empty()
    );
    assert_eq!(
        registry
            .render(&LoopEvent::Other {
                kind: EventKind::new("skill.loaded"),
                payload: serde_json::json!({ "skill": "review" }),
            })
            .len(),
        1
    );
}

/// A bound surface goes through the same store, differ and seal as any other.
/// A view binding is not a second surface path.
#[test]
fn a_bound_surface_is_an_ordinary_surface() {
    let registry = floor_registry();
    let mut store = SurfaceStore::new();
    let turn = TurnId::new();
    let id = orrery_proto::SurfaceId::new();

    let streaming = registry.render(&LoopEvent::AssistantText {
        text: "the workspace".into(),
        complete: false,
    });
    store
        .emit(turn, id, streaming[0].surface.clone())
        .expect("emits");

    let finished = registry.render(&LoopEvent::AssistantText {
        text: "the workspace has three crates".into(),
        complete: true,
    });
    let patches = store
        .emit(turn, id, finished[0].surface.clone())
        .expect("emits");
    assert_eq!(
        patches.len(),
        3,
        "the status, the complete flag and the appended words: {patches:?}"
    );
    assert!(
        patches
            .iter()
            .any(|p| matches!(p, orrery_proto::SurfacePatch::Append { .. })),
        "the prose took the append fast path: {patches:?}"
    );
}
