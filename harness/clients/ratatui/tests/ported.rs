//! Phase 4, in this renderer: three ported extensions, drawn.
//!
//! §8's acceptance criterion is that three ported extensions render **with no
//! drawing code of their own**. `orrery-ported` runs them for real — manifest,
//! host, dispatch, differ, encoder — and hands over the frames. All this file
//! does is draw them, which is the point: the extension said `table`, and the
//! decision to draw a table with these borders, at this width, in this palette
//! was taken here and nowhere else.

use orrery_client_ratatui::app::App;
use orrery_client_ratatui::scrollback::{Recording, lines_of};
use orrery_ported::{examples, run_all};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

/// Every ported extension draws, and the screen is snapshotted.
#[tokio::test]
async fn every_ported_extension_renders() {
    let ported = run_all().await;
    assert_eq!(ported.len(), 3, "three, per §8");
    for run in &ported {
        let mut app = App::new(60);
        let mut sink = Recording::new(60);
        for frame in &run.frames {
            app.apply(frame);
            app.flush_scrollback(&mut sink).expect("prints");
        }
        let mut buf = Buffer::empty(Rect::new(0, 0, 60, 18));
        app.draw(&mut buf);

        let mut screen = String::new();
        if !sink.blocks().is_empty() {
            screen.push_str("-- scrollback --\n");
            screen.push_str(&sink.text());
            screen.push('\n');
        }
        screen.push_str("-- live --\n");
        screen.push_str(&lines_of(&buf).join("\n"));
        insta::assert_snapshot!(run.name.clone(), screen);
    }
}

/// The custom surface draws its fallback here, because this renderer has no
/// timeline widget — and a client that drew neither the payload nor the
/// fallback would be leaving a hole in the transcript (§6.3).
#[tokio::test]
async fn the_custom_surface_falls_back() {
    let example = examples()
        .into_iter()
        .find(|e| e.name == "ported-release-train")
        .expect("the release train is one of the three");
    let run = orrery_ported::run(&example).await;

    let mut app = App::new(60);
    let mut sink = Recording::new(60);
    for frame in &run.frames {
        app.apply(frame);
        app.flush_scrollback(&mut sink).expect("prints");
    }
    let mut buf = Buffer::empty(Rect::new(0, 0, 60, 18));
    app.draw(&mut buf);
    let drawn = format!("{}\n{}", sink.text(), lines_of(&buf).join("\n"));

    assert!(
        drawn.contains("stages shipped"),
        "the fallback's headline is what stands in for the timeline: {drawn}"
    );
    assert!(
        !drawn.contains("[no renderer"),
        "a hole where a custom surface was: {drawn}"
    );
}
