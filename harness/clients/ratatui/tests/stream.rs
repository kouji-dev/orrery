//! Task 4: streaming — plain until complete, and one redraw per budget.

use orrery_agui::Frame;
use orrery_client::{SurfaceStore, SurfaceView};
use orrery_client_ratatui::app::App;
use orrery_client_ratatui::scrollback::{Recording, lines_of};
use orrery_client_ratatui::testing::as_surface;
use orrery_client_ratatui::theme::Theme;
use orrery_client_ratatui::widgets;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn frame(value: serde_json::Value) -> Frame {
    serde_json::from_value(value).expect("a frame")
}

fn draw(view: &SurfaceView, width: u16) -> String {
    let surface = as_surface(view);
    let height = widgets::measure(&surface, width).max(1);
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    widgets::render(&surface, buf.area, &mut buf, &Theme::colour());
    lines_of(&buf).join("\n")
}

#[test]
fn plain_until_complete() {
    let mut store = SurfaceStore::new();
    for value in [
        serde_json::json!({"seq":1,"type":"RUN_STARTED","threadId":"s","runId":"t"}),
        serde_json::json!({"seq":2,"type":"TEXT_MESSAGE_START","messageId":"m","role":"assistant"}),
        serde_json::json!({"seq":3,"type":"TEXT_MESSAGE_CONTENT","messageId":"m","delta":"Here is the fix:\n\n```rust\nfn main() {\n"}),
    ] {
        store.apply(&frame(value));
    }
    let mid = draw(
        store.turn("t").and_then(|t| t.surface("m")).expect("the message"),
        60,
    );
    assert!(
        mid.contains("```rust"),
        "mid-stream the fence is still there, because the source is not markdown yet: {mid}"
    );

    for value in [
        serde_json::json!({"seq":4,"type":"TEXT_MESSAGE_CONTENT","messageId":"m","delta":"    println!(\"hi\");\n}\n```\n"}),
        serde_json::json!({"seq":5,"type":"TEXT_MESSAGE_END","messageId":"m"}),
    ] {
        store.apply(&frame(value));
    }
    let done = draw(
        store.turn("t").and_then(|t| t.surface("m")).expect("the message"),
        60,
    );
    assert!(
        !done.contains("```"),
        "once complete, the fence is drawn rather than printed: {done}"
    );
    assert!(
        done.contains("println!"),
        "and the code is still there: {done}"
    );
    insta::assert_snapshot!("stream-midstream", mid);
    insta::assert_snapshot!("stream-complete", done);
}

#[test]
fn frame_budget() {
    let mut app = App::new(60);
    let mut sink = Recording::new(60);
    app.apply(&frame(
        serde_json::json!({"seq":1,"type":"RUN_STARTED","threadId":"s","runId":"t"}),
    ));
    app.apply(&frame(
        serde_json::json!({"seq":2,"type":"TEXT_MESSAGE_START","messageId":"m","role":"assistant"}),
    ));
    for n in 0..1000u64 {
        app.apply(&frame(serde_json::json!({
            "seq": 3 + n,
            "type": "TEXT_MESSAGE_CONTENT",
            "messageId": "m",
            "delta": "x",
        })));
        app.flush_scrollback(&mut sink).expect("prints");
    }
    assert_eq!(app.draws(), 0, "applying a frame does not draw");

    let mut buf = Buffer::empty(Rect::new(0, 0, 60, 10));
    assert!(app.draw_if_dirty(&mut buf), "there was something to draw");
    assert_eq!(
        app.draws(),
        1,
        "a thousand appends inside one budget cost one redraw, not a thousand"
    );
    assert!(
        !app.draw_if_dirty(&mut buf),
        "and nothing changed since, so the next tick draws nothing"
    );
    assert_eq!(app.draws(), 1);
    assert!(
        sink.blocks().is_empty(),
        "the turn never settled, so scrollback is untouched"
    );
}
