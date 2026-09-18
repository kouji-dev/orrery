//! Phase 4, in the renderer whose reader is not a person.
//!
//! §8 asks for three ported extensions rendering in both TUIs **and** in
//! `--json`. The TUIs snapshot pictures; this one asserts the payload, and the
//! mandatory fallback beside it — which is where §6.2's "a lazy fallback shows
//! up in CI" actually happens.

use orrery_client_json::{FALLBACK_LINE, JsonRenderer};
use orrery_ported::{examples, run, run_all};

fn lines(frames: &[orrery_agui::Frame]) -> Vec<serde_json::Value> {
    let mut renderer = JsonRenderer::new(Vec::new());
    renderer.emit_all(frames).expect("writes");
    String::from_utf8(renderer.into_inner())
        .expect("utf-8")
        .lines()
        .map(|l| serde_json::from_str(l).expect("each line is JSON"))
        .collect()
}

/// Every ported extension comes out as JSON, and every frame it produced is
/// there: this renderer omits nothing, because the comparable rendering is the
/// one that omits nothing.
#[tokio::test]
async fn every_ported_extension_emits() {
    for ported in run_all().await {
        let out = lines(&ported.frames);
        let events = out.iter().filter(|l| l["t"] != FALLBACK_LINE).count();
        assert_eq!(
            events,
            ported.frames.len(),
            "{}: a frame went missing",
            ported.name
        );
        assert_eq!(out[0]["type"], "RUN_STARTED", "{}", ported.name);
        assert_eq!(
            out.last().expect("a last line")["type"],
            "RUN_FINISHED",
            "{}",
            ported.name
        );
    }
}

/// The custom surface emits its payload verbatim **and** its fallback,
/// rendered. This is the line a reviewer reads in CI, and the reason a lazy
/// fallback cannot hide behind "the ADE draws it properly".
#[tokio::test]
async fn the_custom_surface_emits_payload_and_fallback() {
    let example = examples()
        .into_iter()
        .find(|e| e.name == "ported-release-train")
        .expect("the release train is one of the three");
    let ported = run(&example).await;
    let out = lines(&ported.frames);

    let fallback = out
        .iter()
        .find(|l| l["t"] == FALLBACK_LINE)
        .expect("a custom surface produced a fallback line");
    assert_eq!(fallback["kind"], "example-release-train.timeline");

    let stages = fallback["payload"]["stages"]
        .as_array()
        .expect("the payload, verbatim");
    assert_eq!(stages.len(), 5);

    let text = fallback["fallback_text"].as_str().expect("rendered");
    for stage in stages {
        let label = stage["label"].as_str().expect("a label");
        assert!(
            text.contains(label),
            "the fallback drops `{label}`: {text}"
        );
    }
    assert!(
        text.contains("stages shipped"),
        "and it says where the train is: {text}"
    );
    assert!(
        text.len() >= 16 && !text.to_lowercase().contains("open the"),
        "6.2's nudge, asserted rather than trusted: {text}"
    );
}

/// The two extensions with no custom surface emit no fallback line: the
/// mandatory fallback is a property of `custom`, not a decoration on every
/// surface.
#[tokio::test]
async fn a_core_surface_needs_no_fallback_line() {
    for name in ["ported-workspace-census", "ported-patch-review"] {
        let example = examples()
            .into_iter()
            .find(|e| e.name == name)
            .expect("one of the three");
        let ported = run(&example).await;
        assert!(
            !lines(&ported.frames)
                .iter()
                .any(|l| l["t"] == FALLBACK_LINE),
            "{name} emitted a fallback line for a core surface"
        );
    }
}
