//! Task 5: consent is the client's chrome; a question is the extension's.

use crossterm::event::{KeyCode, KeyEvent};
use orrery_agui::Frame;
use orrery_client::conformance::{fixtures_dir, load};
use orrery_client_ratatui::app::App;
use orrery_client_ratatui::event::Outgoing;
use orrery_client_ratatui::scrollback::lines_of;
use orrery_client_ratatui::testing::{as_surface, frames_of};
use orrery_client_ratatui::theme::Theme;
use orrery_client_ratatui::widgets;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn frame(value: serde_json::Value) -> Frame {
    serde_json::from_value(value).expect("a frame")
}

/// An app part-way through the consent fixture: the prompt is up, unanswered.
fn app_awaiting_consent() -> App {
    let scenario = load(&fixtures_dir().join("consent-prompt.jsonl")).expect("fixture loads");
    let mut app = App::new(60);
    for frame in frames_of(&scenario) {
        // Stop at the request; the fixture resolves it two frames later.
        let raised = app.prompt().is_some();
        if raised {
            break;
        }
        app.apply(&frame);
    }
    assert!(app.prompt().is_some(), "the fixture raised a prompt");
    app
}

fn screen(app: &mut App, width: u16, height: u16) -> String {
    let mut buf = Buffer::empty(Rect::new(0, 0, width, height));
    app.draw(&mut buf);
    lines_of(&buf).join("\n")
}

#[test]
fn consent_is_distinct_from_question() {
    let mut app = app_awaiting_consent();
    let consent = screen(&mut app, 60, 16);

    let scenario = load(&fixtures_dir().join("question-surface.jsonl")).expect("fixture loads");
    let mut asked = App::new(60);
    for frame in frames_of(&scenario) {
        if matches!(frame.event, orrery_agui::AguiEvent::RunFinished { .. }) {
            break;
        }
        asked.apply(&frame);
    }
    let question = screen(&mut asked, 60, 16);

    assert_ne!(consent, question, "the two are not drawn the same way");
    assert!(
        consent.contains("consent") && consent.contains("rm -rf target"),
        "the consent bar is the client's own chrome and names the rule: {consent}"
    );
    assert!(
        consent.contains("policy engine"),
        "and says who minted it, which is never an extension: {consent}"
    );
    assert!(
        question.contains("asked by q-1") && question.contains("Which crate?"),
        "a question is attributed to the surface that asked it: {question}"
    );
    assert!(
        !question.contains("consent"),
        "and nothing an extension emits can draw the consent bar: {question}"
    );
    insta::assert_snapshot!("consent-bar", consent);
    insta::assert_snapshot!("question-inline", question);
}

#[test]
fn deadline_counts_down_and_locks() {
    let mut app = app_awaiting_consent();
    let prompt = app.prompt().cloned().expect("a prompt");
    assert_eq!(prompt.deadline_ms, 30_000);

    let fresh = screen(&mut app, 60, 16);
    assert!(fresh.contains("30s"), "the countdown starts full: {fresh}");
    assert!(
        fresh.contains("[a] allow once"),
        "and it is taking answers: {fresh}"
    );

    app.advance(29_000);
    let late = screen(&mut app, 60, 16);
    assert!(late.contains("1s"), "it counts down: {late}");

    // `a` still answers while there is time left.
    let answered = app.key(KeyEvent::from(KeyCode::Char('a')));
    assert_eq!(
        answered,
        vec![Outgoing::Consent {
            prompt: prompt.id.clone(),
            answer: orrery_proto::ConsentAnswerKind::AllowOnce,
        }]
    );

    app.advance(2_000);
    let expired = screen(&mut app, 60, 16);
    assert!(
        expired.contains("the deadline passed"),
        "past the deadline it says so: {expired}"
    );
    assert!(
        !expired.contains("[a] allow once"),
        "and stops offering keys, because the kernel already used the fallback: {expired}"
    );
    assert!(
        app.key(KeyEvent::from(KeyCode::Char('a'))).is_empty(),
        "an answer typed after the deadline is not sent"
    );
    insta::assert_snapshot!("consent-expired", expired);
}

#[test]
fn answer_becomes_an_intent() {
    let mut app = App::new(60);
    for value in [
        serde_json::json!({"seq":1,"type":"RUN_STARTED","threadId":"s","runId":"t"}),
        serde_json::json!({"seq":2,"type":"STATE_DELTA","delta":[{"op":"replace","path":"/surfaces/q-1","value":{"id":"q-1","status":"running","kind":{"t":"question","prompt":"Which crate?","multi":false,"free":false,"choices":[{"value":"proto","label":"orrery-proto"},{"value":"kernel","label":"orrery-kernel"}]}}}]}),
    ] {
        app.apply(&frame(value));
    }

    // Move the cursor, then answer.
    assert!(app.key(KeyEvent::from(KeyCode::Down)).is_empty());
    let out = app.key(KeyEvent::from(KeyCode::Enter));
    assert_eq!(
        out,
        vec![Outgoing::Intent {
            surface: "q-1".into(),
            value: serde_json::json!({ "choice": "kernel" }),
        }],
        "the choice travels back as an intent the kernel validates, not as a state write"
    );
}

#[test]
fn a_consent_bar_swallows_the_composers_keys() {
    let mut app = app_awaiting_consent();
    // Typing under a live consent bar must not land in the composer: the bar is
    // modal, and a stray keystroke there is an answer to a question nobody read.
    assert!(app.key(KeyEvent::from(KeyCode::Char('z'))).is_empty());
    assert_eq!(app.composer().text(), "");
}

#[test]
fn a_question_widget_is_not_a_consent_bar() {
    let surface = as_surface(&orrery_client::SurfaceView {
        id: "q-1".into(),
        status: Some(orrery_proto::Status::Running),
        kind: orrery_proto::SurfaceKind::Question {
            prompt: "Which crate?".into(),
            choices: vec![orrery_proto::Choice {
                value: "proto".into(),
                label: "orrery-proto".into(),
            }],
            multi: false,
            free: false,
            default: None,
            deadline_ms: None,
        },
    });
    let mut buf = Buffer::empty(Rect::new(0, 0, 60, 4));
    widgets::render(&surface, buf.area, &mut buf, &Theme::colour());
    let drawn = lines_of(&buf).join("\n");
    assert!(
        !drawn.contains("consent") && !drawn.contains('┌'),
        "the bordered bar belongs to the policy engine alone: {drawn}"
    );
}
