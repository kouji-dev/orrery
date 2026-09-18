//! Task 2: the scrollback / live split.

use orrery_agui::Frame;
use orrery_client::conformance::{fixtures_dir, load};
use orrery_client_ratatui::app::App;
use orrery_client_ratatui::scrollback::{Recording, lines_of};
use orrery_client_ratatui::testing::frames_of;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn text_turn(seq_base: u64, run: &str, body: &str) -> Vec<Frame> {
    let raw = [
        serde_json::json!({"seq": seq_base, "type": "RUN_STARTED", "threadId": "sess-1", "runId": run}),
        serde_json::json!({"seq": seq_base + 1, "type": "TEXT_MESSAGE_START", "messageId": format!("{run}-msg"), "role": "assistant"}),
        serde_json::json!({"seq": seq_base + 2, "type": "TEXT_MESSAGE_CONTENT", "messageId": format!("{run}-msg"), "delta": body}),
        serde_json::json!({"seq": seq_base + 3, "type": "TEXT_MESSAGE_END", "messageId": format!("{run}-msg")}),
        serde_json::json!({"seq": seq_base + 4, "type": "RUN_FINISHED", "threadId": "sess-1", "runId": run, "result": null}),
    ];
    raw.into_iter()
        .map(|v| serde_json::from_value(v).expect("a frame"))
        .collect()
}

fn live_buffer(app: &mut App, width: u16, height: u16) -> Vec<String> {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    app.draw(&mut buf);
    lines_of(&buf)
}

#[test]
fn settled_turn_is_printed_once() {
    let mut app = App::new(40);
    let mut sink = Recording::new(40);

    for frame in text_turn(1, "turn-a", "the first answer") {
        app.apply(&frame);
    }
    app.flush_scrollback(&mut sink).expect("prints");
    for frame in text_turn(6, "turn-b", "the second answer") {
        app.apply(&frame);
    }
    app.flush_scrollback(&mut sink).expect("prints");
    // Flushing again must not reprint anything: scrollback is write-once.
    app.flush_scrollback(&mut sink).expect("prints");

    assert_eq!(
        sink.blocks().len(),
        2,
        "one insert_before per settled turn, no more: {:?}",
        sink.blocks()
    );
    assert_eq!(app.printed(), ["turn-a", "turn-b"]);
    assert!(sink.text().contains("the first answer"));
    assert!(sink.text().contains("the second answer"));

    let live = live_buffer(&mut app, 40, 8).join("\n");
    assert!(
        !live.contains("the first answer") && !live.contains("the second answer"),
        "a settled turn is gone from the live region: {live}"
    );
}

#[test]
fn live_turn_redraws() {
    let mut app = App::new(40);
    let mut sink = Recording::new(40);
    let frames = text_turn(1, "turn-a", "half an ans");

    // Everything but RUN_FINISHED: the turn is still in flight.
    for frame in frames.iter().take(3) {
        app.apply(frame);
    }
    app.flush_scrollback(&mut sink).expect("prints");
    let first = live_buffer(&mut app, 40, 8);
    assert!(
        first.join("\n").contains("half an ans"),
        "the streaming turn is in the live region: {first:?}"
    );
    assert!(
        sink.blocks().is_empty(),
        "nothing settled, so nothing was printed"
    );

    // A second redraw of the same live turn still touches no scrollback.
    let before = app.draws();
    let second = live_buffer(&mut app, 40, 8);
    assert_eq!(second, first, "the live region redraws the same thing");
    assert_eq!(app.draws(), before + 1, "and it really did redraw");
    assert!(sink.blocks().is_empty(), "scrollback is untouched");
}

#[test]
fn resize_does_not_reflow_history() {
    let mut app = App::new(40);
    let mut sink = Recording::new(40);
    for frame in text_turn(
        1,
        "turn-a",
        "a settled answer that is long enough to wrap somewhere",
    ) {
        app.apply(&frame);
    }
    app.flush_scrollback(&mut sink).expect("prints");
    let printed = sink.blocks().len();

    app.resize(20);
    app.flush_scrollback(&mut sink).expect("prints");

    assert_eq!(
        sink.blocks().len(),
        printed,
        "a resize emits nothing: the terminal owns the reflow of what is already out"
    );

    // The live region is what re-wraps. Draw the in-flight turn at two widths
    // and assert only that it is drawn to the width it was asked for.
    for frame in text_turn(
        6,
        "turn-b",
        "a live answer long enough to need two lines at twenty",
    ) {
        app.apply(&frame);
        if matches!(
            frame.event,
            orrery_agui::AguiEvent::TextMessageContent { .. }
        ) {
            break;
        }
    }
    let narrow = live_buffer(&mut app, 20, 10);
    assert!(
        narrow.iter().all(|l| l.chars().count() <= 20),
        "the live region wrapped to the new width: {narrow:?}"
    );
}

#[test]
fn a_whole_scenario_settles_into_scrollback() {
    let scenario = load(&fixtures_dir().join("tool-call.jsonl")).expect("fixture loads");
    let mut app = App::new(60);
    let mut sink = Recording::new(60);
    for frame in frames_of(&scenario) {
        app.apply(&frame);
        app.flush_scrollback(&mut sink).expect("prints");
    }
    assert_eq!(sink.blocks().len(), 1, "one turn, printed once");
}
