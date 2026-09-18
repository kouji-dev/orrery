//! Task 6: the composer.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use orrery_agui::Frame;
use orrery_client_ratatui::app::App;
use orrery_client_ratatui::composer::{Action, Composer};
use orrery_client_ratatui::event::Outgoing;

fn frame(value: serde_json::Value) -> Frame {
    serde_json::from_value(value).expect("a frame")
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn typed(text: &str, composer: &mut Composer) {
    for c in text.chars() {
        composer.key(KeyEvent::from(KeyCode::Char(c)), false);
    }
}

#[test]
fn ctrl_c_cancels_without_exiting() {
    let mut app = App::new(60);
    app.apply(&frame(
        serde_json::json!({"seq":1,"type":"RUN_STARTED","threadId":"s","runId":"t"}),
    ));

    let out = app.key(ctrl('c'));
    assert_eq!(
        out,
        vec![Outgoing::Cancel],
        "^C during a turn stops the turn"
    );
    assert!(
        app.running(),
        "and the client is still running: cancelling a cargo build must not \
         throw away the transcript"
    );

    // ^D is the exit, and it is a different key on purpose.
    let out = app.key(ctrl('d'));
    assert_eq!(out, vec![Outgoing::Exit]);
    assert!(!app.running());
}

#[test]
fn ctrl_c_outside_a_turn_clears_the_line() {
    let mut app = App::new(60);
    for c in "half a thought".chars() {
        app.key(KeyEvent::from(KeyCode::Char(c)));
    }
    assert_eq!(app.composer().text(), "half a thought");
    assert!(app.key(ctrl('c')).is_empty(), "nothing to cancel");
    assert_eq!(app.composer().text(), "");
    assert!(app.running());
}

#[test]
fn bracketed_paste_does_not_submit() {
    let mut composer = Composer::new();
    typed("here: ", &mut composer);
    // What crossterm hands over between the paste brackets: one block, newlines
    // and all. Without bracketed paste this would have arrived as keystrokes and
    // the first Enter would have submitted one line of three.
    composer.paste("fn main() {\n    println!(\"hi\");\n}");
    assert_eq!(
        composer.text(),
        "here: fn main() {\n    println!(\"hi\");\n}",
        "a multi-line paste lands as one buffer"
    );
    assert_eq!(composer.height(), 3, "and the composer grows to hold it");

    // It submits when the person says so, once, whole.
    let action = composer.key(KeyEvent::from(KeyCode::Enter), false);
    assert_eq!(
        action,
        Action::Send(Outgoing::Submit(
            "here: fn main() {\n    println!(\"hi\");\n}".into()
        ))
    );
}

#[test]
fn shift_enter_is_a_newline_not_a_submit() {
    let mut composer = Composer::new();
    typed("one", &mut composer);
    let action = composer.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT), false);
    assert_eq!(action, Action::None);
    typed("two", &mut composer);
    assert_eq!(composer.text(), "one\ntwo");
}

#[test]
fn history() {
    let mut composer = Composer::new();
    typed("first", &mut composer);
    composer.key(KeyEvent::from(KeyCode::Enter), false);
    typed("second", &mut composer);
    composer.key(KeyEvent::from(KeyCode::Enter), false);
    assert_eq!(composer.history(), ["first", "second"]);
    assert_eq!(composer.text(), "");

    typed("a draft", &mut composer);
    composer.key(KeyEvent::from(KeyCode::Up), false);
    assert_eq!(composer.text(), "second", "up walks back");
    composer.key(KeyEvent::from(KeyCode::Up), false);
    assert_eq!(composer.text(), "first");
    composer.key(KeyEvent::from(KeyCode::Up), false);
    assert_eq!(composer.text(), "first", "and stops at the oldest");
    composer.key(KeyEvent::from(KeyCode::Down), false);
    assert_eq!(composer.text(), "second");
    composer.key(KeyEvent::from(KeyCode::Down), false);
    assert_eq!(
        composer.text(),
        "a draft",
        "walking back out restores what was being typed, rather than eating it"
    );

    // An empty line is not history.
    composer.key(ctrl('u'), false);
    assert_eq!(
        composer.key(KeyEvent::from(KeyCode::Enter), false),
        Action::None
    );
    assert_eq!(composer.history().len(), 2);
}

#[test]
fn ctrl_l_asks_for_a_redraw() {
    let mut composer = Composer::new();
    assert_eq!(composer.key(ctrl('l'), false), Action::Redraw);
}

#[test]
fn a_submit_travels_through_the_session() {
    // The composer never talks to the kernel: it produces an Outgoing and the
    // loop hands it to AguiSession::submit, which owns seq and the pending
    // queue - so a dropped connection resumes the turn instead of losing it.
    let mut app = App::new(60);
    for c in "explain this crate".chars() {
        app.key(KeyEvent::from(KeyCode::Char(c)));
    }
    let out = app.key(KeyEvent::from(KeyCode::Enter));
    assert_eq!(out, vec![Outgoing::Submit("explain this crate".into())]);
    assert_eq!(app.composer().text(), "", "and the line is cleared");
}
