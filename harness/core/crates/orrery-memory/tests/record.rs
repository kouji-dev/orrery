//! Task 5 · Whatever memory injected is recorded in the turn as **resolved
//! content**, never as a pointer. The store moves on; the tree still has to
//! show what the model saw.

mod common;

use std::sync::Arc;

use common::{budget, entry, lifecycle, root_actor};
use orrery_memory::testing::InMemoryProvider;
use orrery_memory::{MemScope, MemoryKernel};
use orrery_proto::{BranchId, ContentBlock, Message, Seq, SessionId, TurnId};
use orrery_session::{CharsOverFour, TurnKind, TurnRow, materialise};

fn row(branch: BranchId, seq: u64, kind: TurnKind) -> TurnRow {
    TurnRow {
        id: TurnId::new(),
        branch,
        seq: Seq(seq),
        kind,
        created_at: 0,
    }
}

fn text_of(messages: &[Message]) -> String {
    messages
        .iter()
        .flat_map(|m| m.content.iter())
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn recalled_is_in_the_turn() {
    let session = SessionId::new();
    let branch = BranchId::new();
    let actor = root_actor(session, branch);

    // --- first half: the turn that actually ran -----------------------------
    let before = Arc::new(InMemoryProvider::anything("before"));
    let kernel = MemoryKernel::new()
        .with_provider(before.clone())
        .with_allowance(budget(10_000));
    kernel
        .write(
            &lifecycle(session, branch).witness(),
            &actor,
            MemScope::Global,
            entry("pref", "the user prefers tabs"),
        )
        .await
        .expect("write");

    let recalls = kernel.recall(&actor, "prefers").await;
    let mut rows = vec![row(
        branch,
        1,
        TurnKind::User {
            input: orrery_proto::UserInput::text("what do I prefer?"),
        },
    )];
    for (i, recall) in recalls.iter().enumerate() {
        rows.push(row(branch, 2 + i as u64, recall.to_turn_kind()));
    }

    // Resolved content, not a query to re-run.
    let recorded = rows
        .iter()
        .find_map(|r| match &r.kind {
            TurnKind::Recalled { provider, entries } => Some((provider.clone(), entries.clone())),
            _ => None,
        })
        .expect("a Recalled row");
    assert_eq!(recorded.0, "before");
    assert_eq!(recorded.1.len(), 1);
    assert_eq!(recorded.1[0].text, "the user prefers tabs");

    let first_pass = materialise(&rows, None, budget(10_000), &CharsOverFour);
    assert!(text_of(&first_pass.messages).contains("the user prefers tabs"));

    // --- second half: the store has moved on --------------------------------
    // A different provider entirely, with different content under the same key.
    let after = Arc::new(InMemoryProvider::anything("after"));
    let replayed_kernel = MemoryKernel::new()
        .with_provider(after.clone())
        .with_allowance(budget(10_000));
    replayed_kernel
        .write(
            &lifecycle(session, branch).witness(),
            &actor,
            MemScope::Global,
            entry("pref", "the user prefers spaces"),
        )
        .await
        .expect("write");

    // The swap is real: a fresh recall now says something else.
    let fresh = replayed_kernel.recall(&actor, "prefers").await;
    assert_eq!(fresh[0].entries[0].text, "the user prefers spaces");

    // The replay does not care. Same rows, same context.
    let second_pass = materialise(&rows, None, budget(10_000), &CharsOverFour);
    assert_eq!(second_pass.messages, first_pass.messages);
    assert!(text_of(&second_pass.messages).contains("the user prefers tabs"));
    assert!(!text_of(&second_pass.messages).contains("spaces"));
}
