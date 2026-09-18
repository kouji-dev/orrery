//! Task 1 · the pure algebra. No database, no async.

use orrery_proto::{MessageRole, Seq, TokenBudget, TurnId, Usage, UserInput};
use orrery_session::algebra::{self, CharsOverFour, TokenCounter};
use orrery_session::turn::{TurnKind, TurnRow};

/// A branch every row in this file belongs to.
fn rows(kinds: Vec<TurnKind>) -> Vec<TurnRow> {
    let branch = orrery_proto::BranchId::new();
    kinds
        .into_iter()
        .enumerate()
        .map(|(i, kind)| TurnRow {
            id: TurnId::new(),
            branch,
            seq: Seq(i as u64 + 1),
            kind,
            created_at: i as i64,
        })
        .collect()
}

fn user(text: &str) -> TurnKind {
    TurnKind::User {
        input: UserInput::text(text),
    }
}

fn assistant(text: &str) -> TurnKind {
    TurnKind::Assistant {
        content: vec![orrery_proto::ContentBlock::Text { text: text.into() }],
        usage: Usage::default(),
    }
}

fn unbounded() -> TokenBudget {
    TokenBudget {
        max: u64::MAX,
        reserve: 0,
    }
}

fn text_of(m: &orrery_proto::Message) -> String {
    m.content
        .iter()
        .filter_map(|b| match b {
            orrery_proto::ContentBlock::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

#[test]
fn materialise_is_oldest_first() {
    let rows = rows(vec![user("one"), assistant("two"), user("three")]);
    let out = algebra::materialise(&rows, None, unbounded(), &CharsOverFour);

    assert_eq!(out.messages.len(), 3, "three turns in, three messages out");
    assert_eq!(text_of(&out.messages[0]), "one");
    assert_eq!(text_of(&out.messages[1]), "two");
    assert_eq!(text_of(&out.messages[2]), "three");
    assert_eq!(out.messages[1].role, MessageRole::Assistant);
    assert!(out.elided.is_empty());
    assert_eq!(out.watermark, None);
}

#[test]
fn elision_never_drops_the_head() {
    // Five turns of 40 characters each: 10 tokens apiece under CharsOverFour.
    let body = "x".repeat(40);
    let rows = rows((1..=5).map(|i| user(&format!("{i}{body}"))).collect());
    // Room for two turns and no more.
    let budget = TokenBudget {
        max: 22,
        reserve: 0,
    };

    let out = algebra::materialise(&rows, None, budget, &CharsOverFour);

    assert_eq!(out.messages.len(), 2, "the budget fits two turns");
    assert!(
        text_of(&out.messages[0]).starts_with('1'),
        "the head is the cached prefix and is never cut"
    );
    assert!(
        text_of(&out.messages[1]).starts_with('5'),
        "the most recent turn is never elided"
    );
    assert_eq!(out.elided.len(), 3, "turns 2, 3 and 4 were taken");
    assert_eq!(
        out.elided,
        vec![rows[1].id, rows[2].id, rows[3].id],
        "elision takes from the middle, oldest first"
    );
}

#[test]
fn watermark_replaces_the_prefix() {
    let mut rows = rows(vec![
        user("one"),
        assistant("two"),
        user("three"),
        assistant("four"),
        user("five"),
    ]);
    rows.push(TurnRow {
        id: TurnId::new(),
        branch: rows[0].branch,
        seq: Seq(6),
        kind: TurnKind::Summary {
            covers: (Seq(1), Seq(3)),
            text: "the first three turns, in brief".into(),
            usage: Usage::default(),
        },
        created_at: 6,
    });

    let out = algebra::materialise(&rows, Some(Seq(3)), unbounded(), &CharsOverFour);

    assert_eq!(out.messages.len(), 3, "summary + turns 4 and 5");
    assert_eq!(text_of(&out.messages[0]), "the first three turns, in brief");
    assert_eq!(text_of(&out.messages[1]), "four");
    assert_eq!(text_of(&out.messages[2]), "five");
    assert_eq!(out.watermark, Some(Seq(3)));
    // The rows themselves are untouched: compaction wrote, it did not mutate.
    assert_eq!(rows.len(), 6);
    assert_eq!(rows[0].seq, Seq(1));
}

#[test]
fn the_most_recent_turn_survives_any_budget() {
    let rows = rows(vec![user(&"a".repeat(400)), user(&"b".repeat(400))]);
    let out = algebra::materialise(
        &rows,
        None,
        TokenBudget { max: 1, reserve: 0 },
        &CharsOverFour,
    );
    assert_eq!(out.messages.len(), 1);
    assert!(text_of(&out.messages[0]).starts_with('b'));
}

#[test]
fn chars_over_four_counts_something() {
    let m = orrery_proto::Message::text(MessageRole::User, "x".repeat(40));
    assert_eq!(CharsOverFour.count(std::slice::from_ref(&m)), 10);
}
