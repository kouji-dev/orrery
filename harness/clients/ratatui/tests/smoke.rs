//! Task 8: the live smoke test, as far as it can be run here.
//!
//! # Why this is not under a pty
//!
//! The plan asks for `orrery serve --provider fixture:turn-with-tool-call.jsonl`
//! driven through a pty with `expectrl` or similar. Two halves of that do not
//! exist yet, and neither is this crate's to add:
//!
//! 1. **No pty crate is pinned in `[workspace.dependencies]`.** `portable-pty`
//!    is in `Cargo.lock` only because the ADE's Tauri tree pulls it in; adding
//!    `expectrl` or `portable-pty` as a pin is a root-manifest change, and the
//!    plan itself says to say so rather than skip silently.
//! 2. **`orrery serve` is not implemented.** `orrery-cli` belongs to plan 17,
//!    and every subcommand there still exits 2 naming the plan that will land
//!    it — including the bare `orrery` that is meant to start this client. So
//!    there is no process for a pty to drive, and nothing is gained by
//!    pretending otherwise.
//!
//! What runs instead is the same assertion one level down: the real event loop,
//! over a real transport (in-memory rather than a pipe, which is exactly the
//! substitution `AguiSession::in_process` is built on), ending with the tool's
//! output on screen. `#[ignore]`d as the plan asks, so it is run explicitly.
//!
//! ```text
//! cargo test -p orrery-client-ratatui --test smoke -- --ignored
//! ```

use orrery_client::conformance::{fixtures_dir, load};
use orrery_client_ratatui::app::App;
use orrery_client_ratatui::event::VecSource;
use orrery_client_ratatui::scrollback::{Recording, lines_of};
use orrery_client_ratatui::testing::frames_of;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[tokio::test]
#[ignore = "the pty half needs a workspace-pinned pty crate and an `orrery serve` that exists; see the module docs"]
async fn one_turn_end_to_end() {
    let scenario = load(&fixtures_dir().join("tool-call.jsonl")).expect("fixture loads");
    let mut app = App::new(80);
    let mut sink = Recording::new(80);

    app.run(&mut VecSource::new(frames_of(&scenario)), &mut sink)
        .await
        .expect("the loop drains and exits cleanly");

    let printed = sink.text();
    assert!(
        printed.contains("builtin.read"),
        "the tool call is in scrollback, where selection works: {printed}"
    );
    assert!(
        printed.contains("Cargo.toml"),
        "with the arguments it was called on: {printed}"
    );
    assert!(
        printed.contains("name = \"orrery\""),
        "and what it produced: {printed}"
    );

    // The turn settled, so the live region is empty apart from the chrome.
    let mut buf = Buffer::empty(Rect::new(0, 0, 80, 10));
    app.draw(&mut buf);
    let live = lines_of(&buf).join("\n");
    assert!(
        !live.contains("builtin.read"),
        "nothing settled is redrawn: {live}"
    );
    assert!(live.contains("^D exit"), "the footer is still there: {live}");
}
