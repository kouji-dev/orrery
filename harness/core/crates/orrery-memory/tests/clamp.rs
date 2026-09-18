//! Task 3 · Recall and the clamp. The token clamp is ours; the store is theirs.

mod common;

use std::sync::Arc;

use common::{budget, entry, lifecycle, root_actor};
use orrery_memory::testing::InMemoryProvider;
use orrery_memory::{CharsOverFour, MemEvent, MemScope, MemoryKernel, TokenCount, split_window};
use orrery_proto::{BranchId, SessionId};

fn long(n: usize) -> String {
    "lorem ipsum ".repeat(n)
}

#[tokio::test]
async fn chatty_provider_is_cut() {
    let provider = Arc::new(InMemoryProvider::anything("chatty"));
    let kernel = MemoryKernel::new()
        .with_provider(provider.clone())
        .with_allowance(budget(2_000));
    let session = SessionId::new();
    let branch = BranchId::new();
    let actor = root_actor(session, branch);
    let w = lifecycle(session, branch);

    // 100k tokens' worth, in 100 entries of ~1k tokens each.
    for i in 0..100 {
        kernel
            .write(
                &w.witness(),
                &actor,
                MemScope::Global,
                entry(&format!("e{i}"), &long(340)),
            )
            .await
            .expect("write");
    }

    let recalls = kernel.recall(&actor, "lorem").await;
    let used: u64 = recalls.iter().map(|r| r.used_tokens).sum();
    let kept: usize = recalls.iter().map(|r| r.entries.len()).sum();
    let dropped: usize = recalls.iter().map(|r| r.dropped).sum();

    assert!(used <= 2_000, "the clamp let {used} tokens through");
    assert!(
        used > 1_000,
        "the clamp threw away an allowance it could fill"
    );
    assert!(kept > 0 && kept < 100, "kept {kept} of 100");
    assert_eq!(kept + dropped, 100);

    // The drop is recorded, not silent.
    let clipped = kernel
        .ledger()
        .into_iter()
        .filter(|e| matches!(e, MemEvent::Clipped { .. }))
        .count();
    assert_eq!(clipped, 1, "a clamp that drops 90 entries has to say so");
}

#[test]
fn history_is_not_evicted() {
    // 100k window, memory's declared share is a tenth of it.
    let window = orrery_proto::TokenBudget {
        max: 100_000,
        reserve: 8_000,
    };
    let split = split_window(window, 0.1);
    assert_eq!(split.memory.available(), 9_200);
    assert_eq!(split.history.available(), 82_800);
    assert_eq!(
        split.memory.available() + split.history.available(),
        window.available(),
    );

    // A provider that would happily fill the whole window still only ever gets
    // its share, so history keeps its own.
    let counter = CharsOverFour;
    let huge = long(100_000);
    assert!(counter.count_text(&huge) > window.max);
    let clamped = orrery_memory::clamp(
        vec![orrery_memory::MemEntry::new("k", &huge)],
        split.memory,
        &counter,
    );
    assert!(clamped.used_tokens <= split.memory.available());
    assert_eq!(
        clamped.dropped, 1,
        "an entry that cannot fit is dropped whole"
    );
}

/// Assembly order, cross-referenced against plan 05's own `ContextDraft`
/// rather than a copy of its field order: recalled memory sits in the volatile
/// suffix, after the tool descriptors, so a memory provider cannot invalidate
/// the prompt cache on every pass.
#[test]
fn order_is_prefix_then_memory() {
    use orrery_kernel::context::{ContextDraft, Section};
    use orrery_proto::{Message, MessageRole, UserInput};

    let recalled = orrery_memory::recalled_message(
        "test",
        &[orrery_session::RecalledEntry {
            key: "pref".to_owned(),
            text: "prefers tabs".to_owned(),
            score: None,
        }],
    )
    .expect("a message");

    let draft = ContextDraft {
        system: vec![
            Section::new("base", "you are a harness"),
            Section::new("agent", "you are the planner"),
            Section::new("tools", "# Tools\n\n## fs.read\nread a file\n"),
            Section::new("skills", "# Skills"),
        ],
        cache_breakpoint: 4,
        recalled: vec![recalled.clone()],
        history: vec![Message::text(MessageRole::Assistant, "earlier")],
        input: orrery_kernel::context::input_message(&UserInput::text("now")),
        tools: Vec::new(),
    };

    // Recalled memory is not in the system prompt at all.
    assert!(!draft.system_text().contains("prefers tabs"));
    // The tool descriptors are inside the cached prefix.
    assert!(draft.section_index("tools").expect("tools") < draft.cache_breakpoint);
    // And the recalled entry is the first thing after it.
    assert_eq!(draft.messages().first(), Some(&recalled));
}
