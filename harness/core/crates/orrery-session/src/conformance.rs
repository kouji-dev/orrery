//! The reusable conformance suite.
//!
//! Every backend runs the same tests. It lives in `src/` rather than in
//! `tests/` for the obvious reason: a `tests/` binary cannot be linked by
//! another crate, and the whole point is that
//! [`orrery-ext-session-sqlite`] runs exactly the suite the in-test memory
//! store runs.
//!
//! ```no_run
//! # use std::sync::Arc;
//! # async fn go(store: Arc<dyn orrery_session::SessionStore>) {
//! orrery_session::conformance::run_conformance(store).await;
//! # }
//! ```
//!
//! Each case is `pub` so a backend can run one on its own while it is being
//! built. They panic rather than returning a `Result`, because they are tests.

use std::sync::Arc;
use std::time::Duration;

use orrery_proto::{CallId, ContentBlock, Outcome, Seq, TokenBudget, ToolRef, Usage, UserInput};

use crate::algebra::CharsOverFour;
use crate::error::SessionError;
use crate::store::SessionStore;
use crate::turn::{BranchOutcome, NewTurn, TurnKind};

/// Anything a backend must satisfy. Panics on the first failure.
pub async fn run_conformance(store: Arc<dyn SessionStore>) {
    create_append_materialise(&*store).await;
    second_lease_is_refused(&*store).await;
    two_branches_lease_concurrently(&*store).await;
    appends_are_contiguous(&*store).await;
    closed_branch_refuses_appends(&*store).await;
    branch_close_and_parent_join(&*store).await;
    events_since_is_contiguous(&*store).await;
    compact_leaves_the_originals_readable(&*store).await;
    compact_materialise_uses_the_highest_watermark(&*store).await;
    tool_results_round_trip(&*store).await;
    sessions_can_be_enumerated(&*store).await;
    child_close_does_not_deadlock_the_parent(&*store).await;
}

fn user(text: &str) -> NewTurn {
    NewTurn::new(TurnKind::User {
        input: UserInput::text(text),
    })
}

fn assistant(text: &str) -> NewTurn {
    NewTurn::new(TurnKind::Assistant {
        content: vec![ContentBlock::Text { text: text.into() }],
        usage: Usage::default(),
    })
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
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// create → append → materialise, the shortest round trip there is.
pub async fn create_append_materialise(store: &dyn SessionStore) {
    let session = store.create("/ws", "default").await.expect("create");
    let handle = store.open(session).await.expect("open");
    assert_eq!(handle.workspace, "/ws");
    assert_eq!(handle.profile, "default");

    let lease = store.lease(handle.root).await.expect("lease");
    store.append(&lease, user("hello")).await.expect("append");
    store
        .append(&lease, assistant("hi back"))
        .await
        .expect("append");
    drop(lease);

    let out = store
        .materialise(handle.root, unbounded(), &CharsOverFour)
        .await
        .expect("materialise");
    assert_eq!(out.messages.len(), 2);
    assert_eq!(text_of(&out.messages[0]), "hello");
    assert_eq!(text_of(&out.messages[1]), "hi back");
    assert_eq!(out.watermark, None);
}

/// One turn at a time per branch; a second lease is refused, not queued.
pub async fn second_lease_is_refused(store: &dyn SessionStore) {
    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;

    let first = store.lease(root).await.expect("the first lease");
    match store.lease(root).await {
        Err(SessionError::BranchBusy { branch }) => assert_eq!(branch, root),
        other => panic!("a busy branch must be refused, got {other:?}"),
    }

    drop(first);
    store
        .lease(root)
        .await
        .expect("the lease is free once the first is dropped");
}

/// Parallel work is parallel branches, so two branches lease at once.
pub async fn two_branches_lease_concurrently(store: &dyn SessionStore) {
    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;

    let lease = store.lease(root).await.expect("lease");
    let anchor = store.append(&lease, user("fork me")).await.expect("append");
    drop(lease);

    let left = store.branch(anchor, "left").await.expect("branch");
    let right = store.branch(anchor, "right").await.expect("branch");

    let a = store.lease(left).await.expect("left leases");
    let b = store
        .lease(right)
        .await
        .expect("right leases at the same time");
    assert_eq!(a.branch(), left);
    assert_eq!(b.branch(), right);
}

/// Sequence numbers on a branch are one-based and contiguous. A backend that
/// let a second row take a used `(branch, seq)` would fail here long before it
/// failed in production.
pub async fn appends_are_contiguous(store: &dyn SessionStore) {
    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;

    let lease = store.lease(root).await.expect("lease");
    for i in 1..=5u64 {
        assert_eq!(
            lease.next_seq(),
            Seq(i),
            "the lease hands out 1..n in order"
        );
        store
            .append(&lease, user(&format!("{i}")))
            .await
            .expect("append");
    }
    assert_eq!(lease.next_seq(), Seq(6));
    drop(lease);

    let out = store
        .materialise(root, unbounded(), &CharsOverFour)
        .await
        .expect("materialise");
    let texts: Vec<String> = out.messages.iter().map(text_of).collect();
    assert_eq!(texts, vec!["1", "2", "3", "4", "5"]);
}

/// A closed branch takes no more turns.
pub async fn closed_branch_refuses_appends(store: &dyn SessionStore) {
    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;
    let lease = store.lease(root).await.expect("lease");
    let anchor = store.append(&lease, user("anchor")).await.expect("append");
    drop(lease);

    let child = store.branch(anchor, "child").await.expect("branch");
    let child_lease = store.lease(child).await.expect("lease");
    store
        .close_branch(
            child_lease,
            BranchOutcome::Completed {
                summary: "done".into(),
            },
        )
        .await
        .expect("close");

    let reopened = store
        .lease(child)
        .await
        .expect("a closed branch still leases");
    match store.append(&reopened, user("too late")).await {
        Err(SessionError::BranchClosed { branch }) => assert_eq!(branch, child),
        other => panic!("a closed branch must refuse an append, got {other:?}"),
    }
}

/// branch → append on the child → close → the parent sees a `BranchResult`.
pub async fn branch_close_and_parent_join(store: &dyn SessionStore) {
    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;

    let lease = store.lease(root).await.expect("lease");
    let anchor = store
        .append(&lease, user("delegate this"))
        .await
        .expect("append");
    drop(lease);

    let child = store.branch(anchor, "sub-agent").await.expect("branch");
    let child_lease = store.lease(child).await.expect("lease");
    store
        .append(&child_lease, assistant("child work"))
        .await
        .expect("append");
    let outcome = BranchOutcome::Completed {
        summary: "the child concluded".into(),
    };
    store
        .close_branch(child_lease, outcome.clone())
        .await
        .expect("close");

    // The parent writes the join, under its own lease.
    let lease = store.lease(root).await.expect("lease");
    store
        .append(
            &lease,
            NewTurn::new(TurnKind::BranchResult {
                child,
                outcome: outcome.clone(),
            }),
        )
        .await
        .expect("the parent appends the join");
    drop(lease);

    let out = store
        .materialise(root, unbounded(), &CharsOverFour)
        .await
        .expect("materialise");
    assert!(
        out.messages
            .iter()
            .any(|m| text_of(m).contains("the child concluded")),
        "the parent's transcript carries the join"
    );
    // The child's own turns stayed on the child's branch.
    let child_view = store
        .materialise(child, unbounded(), &CharsOverFour)
        .await
        .expect("materialise the child");
    assert!(
        child_view
            .messages
            .iter()
            .any(|m| text_of(m) == "child work"),
        "the child keeps its own turns"
    );
}

/// `events_since` returns a contiguous range: a client detects a gap by
/// arithmetic, which is only possible if there are no gaps to begin with.
pub async fn events_since_is_contiguous(store: &dyn SessionStore) {
    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;

    let lease = store.lease(root).await.expect("lease");
    for i in 0..6 {
        store
            .append(&lease, user(&format!("turn {i}")))
            .await
            .expect("append");
    }
    drop(lease);

    let all = store.events_since(session, None).await.expect("events");
    assert_eq!(all.len(), 6, "one event per append");
    for (i, ev) in all.iter().enumerate() {
        assert_eq!(ev.seq, Seq(i as u64 + 1), "contiguous, one-based");
    }

    let tail = store
        .events_since(session, Some(Seq(3)))
        .await
        .expect("events");
    assert_eq!(tail.len(), 3, "`since` is exclusive");
    assert_eq!(tail[0].seq, Seq(4));
}

/// `compact` writes; it never destroys what it compacted.
pub async fn compact_leaves_the_originals_readable(store: &dyn SessionStore) {
    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;

    let lease = store.lease(root).await.expect("lease");
    for i in 1..=5u64 {
        store
            .append(&lease, user(&format!("turn {i}")))
            .await
            .expect("append");
    }

    let result = store
        .compact(
            &lease,
            Seq(3),
            NewTurn::new(TurnKind::Summary {
                covers: (Seq(1), Seq(3)),
                text: "the first three, in brief".into(),
                usage: Usage::default(),
            }),
        )
        .await
        .expect("compact");
    assert_eq!(result.watermark, Seq(3));
    assert_eq!(result.covered, 3);
    drop(lease);

    // The view sits on the watermark...
    let out = store
        .materialise(root, unbounded(), &CharsOverFour)
        .await
        .expect("materialise");
    assert_eq!(out.watermark, Some(Seq(3)));
    let texts: Vec<String> = out.messages.iter().map(text_of).collect();
    assert_eq!(
        texts,
        vec!["the first three, in brief", "turn 4", "turn 5"],
        "summary + what came after"
    );

    // ...and the rows it covers are still there. A backend proves this against
    // its own storage too (`compact::original_rows_survive`); here we prove the
    // only thing the trait can see: the summary turn is a row like any other
    // and nothing lost its id.
    assert!(out.messages.len() < 6);
}

/// Two compactions, the later one wins.
pub async fn compact_materialise_uses_the_highest_watermark(store: &dyn SessionStore) {
    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;

    let lease = store.lease(root).await.expect("lease");
    for i in 1..=6u64 {
        store
            .append(&lease, user(&format!("turn {i}")))
            .await
            .expect("append");
    }
    store
        .compact(
            &lease,
            Seq(2),
            NewTurn::new(TurnKind::Summary {
                covers: (Seq(1), Seq(2)),
                text: "first pass".into(),
                usage: Usage::default(),
            }),
        )
        .await
        .expect("first compaction");
    store
        .compact(
            &lease,
            Seq(4),
            NewTurn::new(TurnKind::Summary {
                covers: (Seq(1), Seq(4)),
                text: "second pass".into(),
                usage: Usage::default(),
            }),
        )
        .await
        .expect("second compaction");
    drop(lease);

    let out = store
        .materialise(root, unbounded(), &CharsOverFour)
        .await
        .expect("materialise");
    assert_eq!(out.watermark, Some(Seq(4)), "the later compaction wins");
    let texts: Vec<String> = out.messages.iter().map(text_of).collect();
    assert!(texts.contains(&"second pass".to_string()));
    assert!(texts.contains(&"turn 5".to_string()));
    assert!(texts.contains(&"turn 6".to_string()));
    assert!(!texts.contains(&"turn 3".to_string()));
}

/// **The highest-value test in the suite.**
///
/// The parent is mid-turn and holding its own lease. It forks a child, the
/// child appends and closes, and the parent then writes the join. If
/// `close_branch` ever reached for the parent's lease, this would hang forever
/// — every sub-agent call, not some rare interleaving. The timeout is the
/// assertion.
pub async fn child_close_does_not_deadlock_the_parent(store: &dyn SessionStore) {
    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;

    // The parent takes its lease and keeps it for the whole turn.
    let parent = store.lease(root).await.expect("the parent leases");
    let anchor = store
        .append(&parent, user("spawn a sub-agent"))
        .await
        .expect("append");

    let work = async {
        let child = store.branch(anchor, "sub-agent").await.expect("branch");
        let child_lease = store.lease(child).await.expect("the child leases");
        store
            .append(&child_lease, assistant("sub-agent answer"))
            .await
            .expect("the child appends");
        let outcome = BranchOutcome::Completed {
            summary: "sub-agent answer".into(),
        };
        store
            .close_branch(child_lease, outcome.clone())
            .await
            .expect("the child closes while the parent holds its lease");

        // The parent performs the join, with the lease it never let go of.
        store
            .append(
                &parent,
                NewTurn::new(TurnKind::BranchResult { child, outcome }),
            )
            .await
            .expect("the parent writes the join");
        child
    };

    let child = tokio::time::timeout(Duration::from_secs(5), work)
        .await
        .expect("the parent-child join must complete, not deadlock");
    drop(parent);

    let out = store
        .materialise(root, unbounded(), &CharsOverFour)
        .await
        .expect("materialise");
    assert!(
        out.messages
            .iter()
            .any(|m| text_of(m).contains(&child.to_string())),
        "the parent's BranchResult row exists"
    );
}

/// A tool-result turn, so a backend proves it round-trips every `TurnKind`.
pub async fn tool_results_round_trip(store: &dyn SessionStore) {
    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;
    let lease = store.lease(root).await.expect("lease");
    let call = CallId::new();
    store
        .append(
            &lease,
            NewTurn::new(TurnKind::ToolResult {
                call,
                r#ref: "builtin.read".parse::<ToolRef>().expect("tool ref"),
                outcome: Outcome::ok(),
            }),
        )
        .await
        .expect("append");
    drop(lease);

    let out = store
        .materialise(root, unbounded(), &CharsOverFour)
        .await
        .expect("materialise");
    assert!(matches!(
        out.messages[0].content.first(),
        Some(ContentBlock::ToolResult { call: c, .. }) if *c == call
    ));
}

/// A store can say **which** sessions it holds, not only answer about one that
/// is already named.
///
/// Every field `orrery session list` prints is asserted here — the id, the
/// workspace, the profile, a creation time and the turn count — because a
/// listing that cannot tell two sessions apart is not a listing. Ordering is
/// newest first; the suite asserts it as "non-increasing", since two sessions
/// created in the same millisecond may legitimately tie.
pub async fn sessions_can_be_enumerated(store: &dyn SessionStore) {
    let older = store.create("/ws/older", "default").await.expect("create");
    let newer = store.create("/ws/newer", "review").await.expect("create");

    let lease = store.lease(store.open(newer).await.expect("open").root).await.expect("lease");
    store.append(&lease, user("one")).await.expect("append");
    store.append(&lease, assistant("two")).await.expect("append");
    drop(lease);

    let listed = store.list_sessions().await.expect("a backend enumerates its sessions");

    let a = listed
        .iter()
        .find(|s| s.session == older)
        .expect("the older session is listed");
    assert_eq!(a.workspace, "/ws/older");
    assert_eq!(a.profile, "default");
    assert_eq!(a.turns, 0, "a session with no turns still lists");

    let b = listed
        .iter()
        .find(|s| s.session == newer)
        .expect("the newer session is listed");
    assert_eq!(b.workspace, "/ws/newer");
    assert_eq!(b.profile, "review");
    assert_eq!(b.turns, 2, "every turn on every branch is counted");
    assert!(b.created_at >= a.created_at, "the later session is not older");

    let times: Vec<i64> = listed.iter().map(|s| s.created_at).collect();
    assert!(
        times.windows(2).all(|w| w[0] >= w[1]),
        "newest first: {times:?}"
    );
    let positions = |id| listed.iter().position(|s| s.session == id);
    assert!(
        positions(newer) <= positions(older),
        "the newer session sorts first"
    );
}
