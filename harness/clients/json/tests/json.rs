//! What the `json` renderer owes a reader who is not a person.

use orrery_agui::{AguiEvent, Frame, PatchOp};
use orrery_client::conformance;
use orrery_client_json::{FALLBACK_LINE, JsonRenderer};

fn lines(frames: &[Frame]) -> Vec<serde_json::Value> {
    let mut renderer = JsonRenderer::new(Vec::new());
    renderer.emit_all(frames).expect("writes");
    String::from_utf8(renderer.into_inner())
        .expect("utf-8")
        .lines()
        .map(|l| serde_json::from_str(l).expect("each line is JSON"))
        .collect()
}

fn custom_surface(placement: Option<&str>) -> Frame {
    let mut value = serde_json::json!({
        "status": "done",
        "kind": {
            "t": "custom",
            "kind": "buildgraph.dag",
            "payload": {"nodes": ["proto", "agui"]},
            "fallback": {"kind": {"t": "text", "value": "proto -> agui"}}
        }
    });
    if let Some(placement) = placement {
        value["placement"] = serde_json::Value::String(placement.to_owned());
        value["kind"]["payload"]["placement"] = serde_json::Value::String(placement.to_owned());
    }
    Frame::new(
        2,
        AguiEvent::StateDelta {
            delta: vec![PatchOp::Replace {
                path: "/surfaces/dag-1".into(),
                value,
            }],
        },
    )
}

fn run_started() -> Frame {
    Frame::new(
        1,
        AguiEvent::RunStarted {
            thread_id: "sess-1".into(),
            run_id: "turn-1".into(),
        },
    )
}

/// A custom surface emits both the payload and its fallback, so a lazy fallback
/// shows up in CI.
#[test]
fn emits_fallback_beside_payload() {
    let out = lines(&[run_started(), custom_surface(None)]);
    let fallback = out
        .iter()
        .find(|l| l["t"] == FALLBACK_LINE)
        .expect("a fallback line");
    assert_eq!(fallback["surface"], "dag-1");
    assert_eq!(fallback["kind"], "buildgraph.dag");
    assert_eq!(
        fallback["payload"]["nodes"],
        serde_json::json!(["proto", "agui"]),
        "the payload, verbatim"
    );
    assert_eq!(
        fallback["fallback_text"], "proto -> agui",
        "and the fallback, rendered, which is the half a person could check"
    );

    // The frame itself is still emitted unchanged: an adapter downstream reads
    // AG-UI, not our summary of it.
    assert_eq!(out[0]["type"], "RUN_STARTED");
    assert_eq!(out[1]["type"], "STATE_DELTA");
}

/// A view binding's `placement` is a human client's concern. The json renderer
/// emits everything.
#[test]
fn ignores_placement() {
    let plain = lines(&[run_started(), custom_surface(None)]);
    let placed = lines(&[run_started(), custom_surface(Some("sidebar"))]);
    assert_eq!(
        plain.len(),
        placed.len(),
        "a placement hint must not change what is emitted"
    );
    assert!(
        placed.iter().any(|l| l["t"] == FALLBACK_LINE),
        "a surface a person would not have been shown is still emitted"
    );
    assert_eq!(
        plain[2]["fallback_text"], placed[2]["fallback_text"],
        "and it is rendered the same way"
    );
}

/// Every conformance scenario renders without losing a frame.
#[test]
fn every_scenario_round_trips() {
    for scenario in conformance::load_all(&conformance::fixtures_dir()).expect("the fixtures load")
    {
        let frames: Vec<Frame> = scenario
            .steps
            .iter()
            .filter_map(|s| match s {
                conformance::Step::Event(frame) => Some((**frame).clone()),
                _ => None,
            })
            .collect();
        let out = lines(&frames);
        let emitted = out.iter().filter(|l| l["t"] != FALLBACK_LINE).count();
        assert_eq!(
            emitted,
            frames.len(),
            "{}: every frame is emitted, hidden or not",
            scenario.name
        );
    }
}
