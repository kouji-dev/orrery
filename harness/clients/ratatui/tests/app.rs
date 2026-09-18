//! Task 1: the skeleton and the event loop.

use orrery_client::conformance::{fixtures_dir, load};
use orrery_client_ratatui::app::App;
use orrery_client_ratatui::event::VecSource;
use orrery_client_ratatui::scrollback::Recording;
use orrery_client_ratatui::testing::frames_of;

#[tokio::test]
async fn attaches_and_drains() {
    let scenario = load(&fixtures_dir().join("text-only.jsonl")).expect("fixture loads");
    let frames = frames_of(&scenario);
    assert_eq!(frames.len(), 6, "the fixture is six frames");

    let mut app = App::new(60);
    let mut sink = Recording::new(60);
    app.run(&mut VecSource::new(frames), &mut sink)
        .await
        .expect("the loop drains and exits cleanly");

    assert!(!app.running(), "the loop exits when the source ends");
    assert_eq!(
        app.store().state().last_seq,
        Some(6),
        "every frame was applied, none skipped"
    );
}

#[tokio::test]
async fn a_panic_restores_the_terminal() {
    // The guard is what restores raw mode, so the test asserts the guard's Drop
    // runs even when the body unwinds - a panic must not leave a terminal raw.
    let restored = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = restored.clone();
    let caught = std::panic::catch_unwind(move || {
        let _guard = orrery_client_ratatui::app::RestoreGuard::new(move || {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        panic!("the draw code exploded");
    });
    assert!(caught.is_err(), "the panic propagates");
    assert!(
        restored.load(std::sync::atomic::Ordering::SeqCst),
        "the terminal was restored on the way out"
    );
}
