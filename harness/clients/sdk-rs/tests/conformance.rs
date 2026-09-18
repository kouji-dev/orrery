//! The fixtures, run against the Rust `SurfaceStore`.
//!
//! The same files run under vitest in `clients/sdk-ts`. If these two ever
//! disagree, one of the five clients is drawing something the others are not.

use orrery_agui::{AguiEvent, Frame};
use orrery_client::conformance::{self, Step};
use orrery_client::{StoreChange, SurfaceStore};
use orrery_proto::{Status, SurfaceKind};

/// Every scenario, from the first checkpoint to the last.
#[test]
fn all_scenarios() {
    let dir = conformance::fixtures_dir();
    let scenarios = conformance::load_all(&dir).expect("the fixtures load");
    assert_eq!(
        scenarios.len(),
        conformance::SCENARIO_COUNT,
        "the conformance set is the contract, and every client asserts the same count: add a scenario and bump `conformance::SCENARIO_COUNT`, or find out which one was deleted"
    );

    let mut failures = Vec::new();
    for scenario in &scenarios {
        assert!(
            scenario.steps.iter().any(|s| matches!(s, Step::Expect(_))),
            "{} asserts nothing",
            scenario.name
        );
        if let Err(why) = conformance::run(scenario) {
            failures.push(why);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

fn scenario(name: &str) -> conformance::Scenario {
    conformance::load(&conformance::fixtures_dir().join(format!("{name}.jsonl")))
        .expect("the fixture loads")
}

/// Deltas accumulate into one markdown surface with `complete: false`, flipped
/// `true` at message end.
#[test]
fn text_deltas_become_markdown() {
    let store = conformance::run(&scenario("streaming-markdown")).expect("it passes");
    let turn = store.turn("turn-3").expect("the turn");
    let surface = turn.surface("msg-1").expect("the message");
    let SurfaceKind::Markdown { value, complete } = &surface.kind else {
        panic!(
            "text deltas must become a markdown surface, not {:?}",
            surface.kind
        );
    };
    assert!(*complete, "flipped true at message end");
    assert!(value.starts_with("Here is the fix:"));
    assert_eq!(value.matches("```").count(), 2, "the fence closed");
    assert_eq!(surface.status, Some(Status::Done));

    // And mid-stream it is explicitly not complete, so a renderer knows not to
    // trust a half-open fence.
    let mut store = SurfaceStore::new();
    for step in &scenario("streaming-markdown").steps {
        match step {
            Step::Event(frame) => {
                store.apply(frame);
                if matches!(frame.event, AguiEvent::TextMessageContent { .. }) {
                    let live = store.live().expect("a live turn");
                    let SurfaceKind::Markdown { complete, .. } =
                        &live.surface("msg-1").expect("the message").kind
                    else {
                        panic!("markdown");
                    };
                    assert!(!complete, "not complete while it is still streaming");
                }
            }
            Step::Unknown { seq, what } => {
                store.apply_unknown(*seq, what);
            }
            Step::Expect(_) => {}
        }
    }
}

/// A lost frame is detected by arithmetic and reported, and the event that
/// revealed it is still applied.
#[test]
fn gap_is_detected() {
    let store = conformance::run(&scenario("seq-gap")).expect("it passes");
    assert_eq!(store.gaps().len(), 1);
    assert_eq!(store.gaps()[0].expected, 4);
    assert_eq!(store.gaps()[0].got, 6);

    // The change is reported at the moment it happens, which is a renderer's
    // cue to re-attach.
    let mut store = SurfaceStore::new();
    store.apply(&Frame::new(
        1,
        AguiEvent::TextMessageStart {
            message_id: "m".into(),
            role: "assistant".into(),
        },
    ));
    let changes = store.apply(&Frame::new(
        4,
        AguiEvent::TextMessageEnd {
            message_id: "m".into(),
        },
    ));
    assert!(
        changes.contains(&StoreChange::GapDetected {
            expected: 2,
            got: 4
        }),
        "got {changes:?}"
    );

    // A *merged* frame is not a gap. A coalesced client would otherwise re-attach
    // every tick.
    let mut store = SurfaceStore::new();
    store.apply(&Frame::new(
        1,
        AguiEvent::TextMessageStart {
            message_id: "m".into(),
            role: "assistant".into(),
        },
    ));
    let merged = Frame {
        seq: 9,
        merged_from: Some(2),
        event: AguiEvent::TextMessageContent {
            message_id: "m".into(),
            delta: "eight frames in one".into(),
        },
    };
    let changes = store.apply(&merged);
    assert!(
        !changes
            .iter()
            .any(|c| matches!(c, StoreChange::GapDetected { .. })),
        "a merge is not a loss: {changes:?}"
    );
}

/// An event type and a `Custom` name this client has never heard of are both
/// ignored rather than rejected, and both still consume their sequence number.
#[test]
fn unknown_events_are_ignored_not_rejected() {
    let store = conformance::run(&scenario("custom-with-fallback")).expect("it passes");
    assert!(store.gaps().is_empty(), "an ignored event is not a gap");
    let store = conformance::run(&scenario("streaming-markdown")).expect("it passes");
    assert!(store.gaps().is_empty());
}

/// A custom surface keeps its payload and its fallback side by side. The store
/// picks neither: that is the renderer's call.
#[test]
fn custom_keeps_payload_and_fallback() {
    let store = conformance::run(&scenario("custom-with-fallback")).expect("it passes");
    let surface = store
        .turn("turn-10")
        .expect("the turn")
        .surface("dag-1")
        .expect("the surface");
    let SurfaceKind::Custom {
        kind,
        payload,
        fallback,
    } = &surface.kind
    else {
        panic!("a custom surface");
    };
    assert_eq!(kind, "buildgraph.dag");
    assert!(payload.get("nodes").is_some());
    assert!(matches!(fallback.kind, SurfaceKind::Text { .. }));
}
