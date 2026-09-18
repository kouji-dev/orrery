//! Consent deadlines (translation #12).

use std::time::Duration;

use orrery_agui::AguiEvent;
use orrery_proto::{Capability, ConsentAnswerKind, ConsentPrompt, Event, PromptId, Seq, Subject};
use orrery_transport::Hub;

fn prompt(id: u128) -> ConsentPrompt {
    ConsentPrompt {
        id: PromptId::from_uuid(uuid::Uuid::from_u128(id)),
        subject: Subject::Ext("builtin".parse().expect("ext id")),
        capabilities: Vec::<Capability>::new(),
        reason: "rm -rf target".into(),
        rule: None,
        surface: None,
    }
}

fn request(seq: u64, id: u128, deadline_ms: u64) -> Event {
    Event::ConsentRequest {
        seq: Seq(seq),
        prompt: prompt(id),
        deadline_ms,
    }
}

/// A prompt whose deadline lapsed while nobody was attached replays as
/// resolved-by-fallback, not as a live question.
///
/// Otherwise a client that reconnects after a minute is asked to approve
/// something the kernel already denied, and whichever way the person answers,
/// the answer is about a decision that has already been made.
#[tokio::test]
async fn expired_prompt_is_not_replayed_live() {
    let hub = Hub::new("sess-1");
    hub.publish(&Event::TurnStarted {
        seq: Seq(1),
        turn: orrery_proto::TurnId::from_uuid(uuid::Uuid::from_u128(0xff)),
    });
    let frames = hub.publish(&request(2, 9, 50));
    let asked_at = frames[0].seq;

    // The frame goes out with the absolute deadline on it, not just the
    // relative one: a client that reconnects has to be able to tell.
    let AguiEvent::Custom { name, value } = &frames[0].event else {
        panic!("consent rides Custom");
    };
    assert_eq!(name, orrery_agui::CONSENT_REQUEST);
    assert_eq!(value["deadline_ms"], 50);
    assert!(
        value["expires_at_mono"].is_u64(),
        "the absolute deadline too"
    );

    // Attaching before the deadline still shows a live question.
    let live = hub.attach(Some(asked_at - 1)).expect("replay");
    assert!(matches!(
        &live[0].event,
        AguiEvent::Custom { name, .. } if name == orrery_agui::CONSENT_REQUEST
    ));

    tokio::time::sleep(Duration::from_millis(80)).await;

    let replayed = hub.attach(Some(asked_at - 1)).expect("replay");
    assert_eq!(
        replayed[0].seq, asked_at,
        "the frame keeps its sequence number"
    );
    let AguiEvent::Custom { name, value } = &replayed[0].event else {
        panic!("still Custom, just a different one");
    };
    assert_eq!(name, orrery_agui::CONSENT_RESOLVED);
    assert_eq!(
        value["answer"], "deny",
        "the fallback denies; it never grants"
    );
    assert_eq!(value["by"], "fallback");
    assert_eq!(value["prompt"]["reason"], "rm -rf target");
}

/// An answer inside the deadline sticks; one after it is ignored, because the
/// fallback already applied.
#[tokio::test]
async fn a_late_answer_does_not_reopen_a_settled_prompt() {
    let hub = Hub::new("sess-1");
    hub.publish(&request(1, 9, 10_000));
    let id = PromptId::from_uuid(uuid::Uuid::from_u128(9));
    assert!(hub.answer(id, ConsentAnswerKind::AllowOnce), "in time");
    assert!(
        !hub.answer(id, ConsentAnswerKind::Deny),
        "and only the first answer counts"
    );

    let replayed = hub.attach(None).expect("replay");
    let AguiEvent::Custom { name, value } = &replayed[0].event else {
        panic!("Custom");
    };
    assert_eq!(name, orrery_agui::CONSENT_RESOLVED);
    assert_eq!(value["answer"], "allow-once");
    assert_eq!(value["by"], "user");

    let hub = Hub::new("sess-2");
    hub.publish(&request(1, 11, 20));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        !hub.answer(
            PromptId::from_uuid(uuid::Uuid::from_u128(11)),
            ConsentAnswerKind::AllowOnce
        ),
        "an answer after the deadline changes nothing"
    );
    let entry = hub
        .consent_state(&PromptId::from_uuid(uuid::Uuid::from_u128(11)).to_string())
        .expect("the prompt is on the ledger");
    assert_eq!(
        entry.resolution(hub.clock().now_ms()),
        Some((ConsentAnswerKind::Deny, "fallback"))
    );
}
